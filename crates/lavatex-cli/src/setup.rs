//! `lavatex setup onlyoffice` and `lavatex setup powerpoint`: one command
//! per office suite that makes LavaTeX work with no manual steps.
//!
//! ONLYOFFICE plugins and PowerPoint add-ins are not allowed to start
//! programs, so rendering with the user's TeX needs the local helper. Setup:
//!
//! 1. copies `lavatex` (and `lavatex-helper`) to a stable per-user folder;
//! 2. ONLYOFFICE: installs the plugin into every ONLYOFFICE Desktop found
//!    (incl. Flatpak and Snap) together with a pairing file, so the plugin
//!    finds the helper and its access token by itself;
//!    PowerPoint: creates the helper's HTTPS certificate, trusts it, and
//!    registers the add-in manifest with PowerPoint;
//! 3. makes the helper start automatically: on demand through systemd socket
//!    activation on Linux (it exits again when idle), at login on macOS
//!    (LaunchAgent) and Windows (Run key, no console window).
//!
//! Both hosts share the helper, its autostart entry and the installed
//! programs; uninstalling one keeps them while the other is still set up.

use crate::embedded;
use crate::serve::{https_port, DEFAULT_IDLE_EXIT, DEFAULT_PORT};
use crate::tls;
use lavatex_core::config::load_or_create_token;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

pub const PLUGIN_GUID: &str = "{6C1B2C4E-8D53-4F0A-9B7E-4A1E5D2F7C31}";
/// Read by the plugin from its own folder to find the helper.
pub const PAIRING_FILE: &str = "lavatex-helper.json";
const SERVICE_NAME: &str = "lavatex";
const LAUNCHD_LABEL: &str = "org.lavatex.helper";
const WINDOWS_RUN_VALUE: &str = "LavaTeX";
/// Id in hosts/powerpoint/manifest.xml.
pub const ADDIN_ID: &str = "f4c7023c-3af1-4a72-a223-8c548c054c85";
const ADDIN_MANIFEST_FILE: &str = "lavatex-powerpoint.xml";
/// Office's per-user list of sideloaded ("developer") add-ins on Windows.
const WEF_DEVELOPER_KEY: &str = r"HKCU\Software\Microsoft\Office\16.0\WEF\Developer";

#[derive(clap::Args)]
pub struct SetupArgs {
    #[command(subcommand)]
    pub target: Target,
}

#[derive(clap::Subcommand)]
pub enum Target {
    /// Install the ONLYOFFICE plugin and make its helper start automatically.
    Onlyoffice(OnlyofficeArgs),
    /// Install the PowerPoint add-in (LavaTeX ribbon tab) and its helper.
    Powerpoint(PowerpointArgs),
}

#[derive(clap::Args)]
pub struct PowerpointArgs {
    /// Remove the add-in, its certificate and (if ONLYOFFICE does not use
    /// them) the autostart entry and the installed programs.
    #[arg(long)]
    pub uninstall: bool,
    /// Do not make the helper start automatically.
    #[arg(long)]
    pub no_autostart: bool,
    /// Make a new HTTPS certificate even if one exists.
    #[arg(long)]
    pub new_certificate: bool,
    /// HTTP port of the helper; the add-in uses HTTPS on the next port.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
}

#[derive(clap::Args)]
pub struct OnlyofficeArgs {
    /// Remove the plugin, the autostart entry and the installed programs.
    #[arg(long)]
    pub uninstall: bool,
    /// ONLYOFFICE `sdkjs-plugins` folder to use instead of detecting it.
    #[arg(long)]
    pub plugins_dir: Option<PathBuf>,
    /// Do not make the helper start automatically.
    #[arg(long)]
    pub no_autostart: bool,
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
}

pub fn run(args: SetupArgs) -> ExitCode {
    match args.target {
        Target::Onlyoffice(a) if a.uninstall => uninstall_onlyoffice(&a),
        Target::Onlyoffice(a) => setup_onlyoffice(&a),
        Target::Powerpoint(a) if a.uninstall => uninstall_powerpoint(&a),
        Target::Powerpoint(a) => setup_powerpoint(&a),
    }
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Where `lavatex` and `lavatex-helper` are copied so autostart entries keep
/// working when the downloaded copy is moved or deleted.
fn install_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("LavaTeX"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/LavaTeX/bin"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
            .map(|d| d.join("lavatex/bin"))
    }
}

/// ONLYOFFICE Desktop installations for this user: (description, the folder
/// ONLYOFFICE keeps its data in, the `sdkjs-plugins` folder inside it).
pub fn onlyoffice_locations(home: &Path) -> Vec<(&'static str, PathBuf, PathBuf)> {
    let mut out = Vec::new();
    let mut add = |label: &'static str, app: PathBuf, data: PathBuf| {
        let plugins = data.join("data").join("sdkjs-plugins");
        out.push((label, app, plugins));
    };
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let data = PathBuf::from(local)
                .join("ONLYOFFICE")
                .join("DesktopEditors");
            add("ONLYOFFICE Desktop Editors", data.clone(), data);
        }
    } else if cfg!(target_os = "macos") {
        let data = home.join("Library/Application Support/asc.onlyoffice.ONLYOFFICE");
        add("ONLYOFFICE Desktop Editors", data.clone(), data);
    } else {
        let xdg = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        let native = xdg.join("onlyoffice/desktopeditors");
        add("ONLYOFFICE Desktop Editors", native.clone(), native);
        let flatpak = home.join(".var/app/org.onlyoffice.desktopeditors");
        add(
            "ONLYOFFICE Desktop Editors (Flatpak)",
            flatpak.clone(),
            flatpak.join("data/onlyoffice/desktopeditors"),
        );
        let snap = home.join("snap/onlyoffice-desktopeditors");
        add(
            "ONLYOFFICE Desktop Editors (Snap)",
            snap.clone(),
            snap.join("current/.local/share/onlyoffice/desktopeditors"),
        );
    }
    out
}

/// The `sdkjs-plugins` folders to install into.
fn target_plugin_dirs(args: &OnlyofficeArgs) -> Vec<(String, PathBuf)> {
    if let Some(dir) = &args.plugins_dir {
        return vec![("the folder you chose".to_string(), dir.clone())];
    }
    let Some(home) = home() else {
        return Vec::new();
    };
    onlyoffice_locations(&home)
        .into_iter()
        .filter(|(_, app, _)| app.is_dir())
        .map(|(label, _, plugins)| (label.to_string(), plugins))
        .collect()
}

pub fn pairing_json(port: u16, token: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "url": format!("http://127.0.0.1:{port}"),
        "token": token,
        "version": lavatex_core::VERSION,
    }))
    .expect("serializes")
}

/// Write the embedded plugin plus the pairing file into `plugins/{GUID}`.
pub fn install_plugin(plugins_dir: &Path, port: u16, token: &str) -> std::io::Result<PathBuf> {
    let dir = plugins_dir.join(PLUGIN_GUID);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    for (rel, bytes) in embedded::PLUGIN_FILES {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
    }
    let pairing = dir.join(PAIRING_FILE);
    std::fs::write(&pairing, pairing_json(port, token))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&pairing, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(dir)
}

/// Copy this program (and the helper next to it) into the install folder.
/// Returns the command line that starts the helper.
fn install_binaries() -> Result<Vec<PathBuf>, String> {
    let current = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("cannot find this program: {e}"))?;
    let src_dir = current.parent().map(Path::to_path_buf).unwrap_or_default();
    let dest_dir = install_dir().ok_or("cannot find a folder to install LavaTeX into")?;
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("cannot create {}: {e}", dest_dir.display()))?;

    if cfg!(windows) {
        // A running helper locks its executable on Windows.
        let _ = Command::new("taskkill")
            .args(["/IM", &exe_name("lavatex-helper"), "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    let mut installed_helper = None;
    for name in ["lavatex", "lavatex-helper"] {
        let src = if name == "lavatex" {
            current.clone()
        } else {
            src_dir.join(exe_name(name))
        };
        if !src.is_file() {
            continue;
        }
        let dest = dest_dir.join(exe_name(name));
        let same = dest.canonicalize().map(|d| d == src).unwrap_or(false);
        if !same {
            // Copy then rename, so a running copy on Linux/macOS is replaced safely.
            let tmp = dest_dir.join(format!(".{}.new", exe_name(name)));
            std::fs::copy(&src, &tmp).map_err(|e| format!("cannot copy {}: {e}", src.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
            }
            std::fs::rename(&tmp, &dest)
                .map_err(|e| format!("cannot install {}: {e}", dest.display()))?;
        }
        if name == "lavatex-helper" {
            installed_helper = Some(dest);
        }
    }
    Ok(match installed_helper {
        Some(helper) => vec![helper],
        None => vec![dest_dir.join(exe_name("lavatex")), PathBuf::from("serve")],
    })
}

/// Minimal HTTP GET against the local helper; returns (status, body).
fn http_get(port: u16, path: &str, token: Option<&str>) -> Option<(u16, String)> {
    http_request("GET", port, path, token)
}

/// Ask a running helper (possibly an older version) to exit, and wait until
/// the port is free.
fn stop_running_helper(port: u16, token: &str) {
    if http_request("POST", port, "/v1/shutdown", Some(token)).is_some() {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && http_get(port, "/v1/health", None).is_some() {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

fn http_request(method: &str, port: u16, path: &str, token: Option<&str>) -> Option<(u16, String)> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .ok()?;
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let status = response.split_whitespace().nth(1)?.parse().ok()?;
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Some((status, body))
}

fn wait_for_helper(port: u16, seconds: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if matches!(http_get(port, "/v1/health", None), Some((200, _))) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

fn spawn_detached(cmd: &[PathBuf]) -> std::io::Result<()> {
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0); // survive the terminal being closed
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    c.spawn().map(|_| ())
}

fn run_quiet(program: &str, args: &[&str]) -> Result<(), String> {
    match Command::new(program).args(args).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

fn quote_cmd(cmd: &[PathBuf]) -> String {
    cmd.iter()
        .map(|p| {
            let s = p.to_string_lossy();
            if s.contains(' ') {
                format!("\"{s}\"")
            } else {
                s.into_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// systemd user units for on-demand start: the socket listens all the time,
/// the helper runs only while it is being used.
pub fn systemd_units(
    helper_cmd: &[PathBuf],
    port: u16,
    https: bool,
    path_env: &str,
) -> (String, String) {
    // The HTTPS socket (PowerPoint add-in) is passed second, as fd 4.
    let https_line = if https {
        format!("ListenStream=127.0.0.1:{}\n", https_port(port))
    } else {
        String::new()
    };
    let socket = format!(
        "[Unit]\nDescription=LavaTeX helper socket (LaTeX rendering for office plugins)\n\n\
         [Socket]\nListenStream=127.0.0.1:{port}\n{https_line}\n[Install]\nWantedBy=sockets.target\n"
    );
    let exec = format!("{} --idle-exit {DEFAULT_IDLE_EXIT}", quote_cmd(helper_cmd));
    let service = format!(
        "[Unit]\nDescription=LavaTeX helper (LaTeX rendering for office plugins)\nRequires={SERVICE_NAME}.socket\n\n\
         [Service]\nExecStart={exec}\nEnvironment=\"PATH={}\"\n",
        path_env.replace('%', "%%").replace('"', "")
    );
    (socket, service)
}

fn systemd_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))
        .map(|d| d.join("systemd/user"))
}

fn xdg_autostart_file() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))
        .map(|d| d.join("autostart/lavatex-helper.desktop"))
}

fn launchd_plist() -> Option<PathBuf> {
    home().map(|h| h.join(format!("Library/LaunchAgents/{LAUNCHD_LABEL}.plist")))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Register automatic start. Returns a sentence describing what was done.
fn register_autostart(helper_cmd: &[PathBuf], port: u16, token: &str) -> Result<String, String> {
    let path_env = std::env::var("PATH").unwrap_or_default();
    // Replace a helper that is already running (for example an older version).
    stop_running_helper(port, token);
    if cfg!(windows) {
        let value = quote_cmd(helper_cmd);
        run_quiet(
            "reg",
            &[
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                WINDOWS_RUN_VALUE,
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ],
        )?;
        spawn_detached(helper_cmd).map_err(|e| e.to_string())?;
        return Ok("starts in the background when you log in to Windows (running now)".into());
    }
    if cfg!(target_os = "macos") {
        let plist = launchd_plist().ok_or("no home folder")?;
        let args: String = helper_cmd
            .iter()
            .map(|a| {
                format!(
                    "    <string>{}</string>\n",
                    xml_escape(&a.to_string_lossy())
                )
            })
            .collect();
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{LAUNCHD_LABEL}</string>\n  <key>ProgramArguments</key>\n  <array>\n{args}  </array>\n\
             \x20 <key>EnvironmentVariables</key>\n  <dict>\n    <key>PATH</key>\n    <string>{}</string>\n  </dict>\n\
             \x20 <key>RunAtLoad</key>\n  <true/>\n</dict>\n</plist>\n",
            xml_escape(&path_env)
        );
        if let Some(parent) = plist.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let plist_str = plist.to_string_lossy().into_owned();
        let _ = run_quiet("launchctl", &["unload", &plist_str]);
        std::fs::write(&plist, text).map_err(|e| e.to_string())?;
        run_quiet("launchctl", &["load", "-w", &plist_str])?;
        return Ok("starts in the background when you log in to macOS (running now)".into());
    }
    // Linux and other Unix: systemd socket activation when available.
    if run_quiet("systemctl", &["--user", "show-environment"]).is_ok() {
        let dir = systemd_dir().ok_or("no config folder")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let https = tls::tls_dir().is_some_and(|d| tls::is_configured(&d));
        let (socket, service) = systemd_units(helper_cmd, port, https, &path_env);
        std::fs::write(dir.join(format!("{SERVICE_NAME}.socket")), socket)
            .map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{SERVICE_NAME}.service")), service)
            .map_err(|e| e.to_string())?;
        run_quiet("systemctl", &["--user", "daemon-reload"])?;
        // Restart so an updated helper is picked up.
        let _ = run_quiet(
            "systemctl",
            &["--user", "stop", &format!("{SERVICE_NAME}.service")],
        );
        run_quiet("systemctl", &["--user", "enable", "--now", &format!("{SERVICE_NAME}.socket")]).map_err(|e| {
            format!(
                "{e}\nIf another program (for example a \"lavatex serve\" you started yourself) uses port {port}, stop it and run this again."
            )
        })?;
        return Ok("starts automatically when needed, and stops again when idle (systemd)".into());
    }
    let file = xdg_autostart_file().ok_or("no config folder")?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &file,
        format!(
            "[Desktop Entry]\nType=Application\nName=LavaTeX helper\nComment=LaTeX rendering for office plugins\nExec={}\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
            quote_cmd(helper_cmd)
        ),
    )
    .map_err(|e| e.to_string())?;
    spawn_detached(helper_cmd).map_err(|e| e.to_string())?;
    Ok("starts in the background when you log in (running now)".into())
}

fn unregister_autostart() -> Vec<String> {
    let mut done = Vec::new();
    if cfg!(windows) {
        if run_quiet(
            "reg",
            &[
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                WINDOWS_RUN_VALUE,
                "/f",
            ],
        )
        .is_ok()
        {
            done.push("removed the login entry".into());
        }
        let _ = run_quiet("taskkill", &["/IM", &exe_name("lavatex-helper"), "/F"]);
        return done;
    }
    if cfg!(target_os = "macos") {
        if let Some(plist) = launchd_plist().filter(|p| p.exists()) {
            let _ = run_quiet("launchctl", &["unload", &plist.to_string_lossy()]);
            let _ = std::fs::remove_file(&plist);
            done.push("removed the LaunchAgent".into());
        }
        return done;
    }
    if let Some(dir) = systemd_dir() {
        let socket = dir.join(format!("{SERVICE_NAME}.socket"));
        if socket.exists() {
            let _ = run_quiet(
                "systemctl",
                &[
                    "--user",
                    "disable",
                    "--now",
                    &format!("{SERVICE_NAME}.socket"),
                    &format!("{SERVICE_NAME}.service"),
                ],
            );
            let _ = std::fs::remove_file(&socket);
            let _ = std::fs::remove_file(dir.join(format!("{SERVICE_NAME}.service")));
            let _ = run_quiet("systemctl", &["--user", "daemon-reload"]);
            done.push("removed the systemd units".into());
        }
    }
    if let Some(file) = xdg_autostart_file().filter(|f| f.exists()) {
        let _ = std::fs::remove_file(file);
        done.push("removed the login entry".into());
    }
    done
}

/// TeX first: without it nothing can render, so say so clearly.
fn report_tex() {
    match lavatex_core::toolchain::detect().into_iter().next() {
        Some(tc) => println!("  ✓ TeX found: {} ({})", tc.label, tc.bin_dir.display()),
        None => println!(
            "  ! No TeX installation found yet. Install TeX Live, MiKTeX or MacTeX; LavaTeX will use it as soon as it is installed."
        ),
    }
}

/// Register autostart, (re)start the helper and check that it answers with
/// our token.
fn start_and_check_helper(helper: &[PathBuf], port: u16, token: &str) -> Result<(), ExitCode> {
    match register_autostart(helper, port, token) {
        Ok(how) => println!("  ✓ Helper {how}"),
        Err(e) => {
            eprintln!("  ✗ Could not set up automatic start: {e}");
            return Err(ExitCode::from(1));
        }
    }
    if !wait_for_helper(port, 10) {
        eprintln!(
            "  ✗ The helper did not answer on port {port}. Try \"lavatex serve\" to see why."
        );
        return Err(ExitCode::from(1));
    }
    match http_get(port, "/v1/toolchains", Some(token)) {
        Some((200, _)) => Ok(()),
        Some((401, _)) => {
            eprintln!(
                "  ✗ Another helper with a different access token is using port {port}. Stop it (or log out and in again) and rerun this command."
            );
            Err(ExitCode::from(1))
        }
        other => {
            eprintln!("  ✗ Unexpected answer from the helper: {other:?}");
            Err(ExitCode::from(1))
        }
    }
}

fn setup_onlyoffice(args: &OnlyofficeArgs) -> ExitCode {
    println!("Setting up LavaTeX for ONLYOFFICE…\n");
    report_tex();

    let token = match load_or_create_token(crate::new_token) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("  ✗ Cannot create the access token: {e}");
            return ExitCode::from(1);
        }
    };

    let targets = target_plugin_dirs(args);
    if targets.is_empty() {
        eprintln!(
            "  ✗ ONLYOFFICE Desktop Editors was not found for this user.\n    Start ONLYOFFICE once and run this again, or give the plugin folder with --plugins-dir."
        );
        return ExitCode::from(1);
    }
    for (label, dir) in &targets {
        match install_plugin(dir, args.port, &token) {
            Ok(path) => println!("  ✓ Plugin installed for {label}\n      {}", path.display()),
            Err(e) => {
                eprintln!("  ✗ Cannot install the plugin into {}: {e}", dir.display());
                return ExitCode::from(1);
            }
        }
    }

    if args.no_autostart {
        println!(
            "  - Automatic start skipped. Run \"lavatex serve\" yourself when you use the plugin."
        );
    } else {
        let helper = match install_binaries() {
            Ok(cmd) => cmd,
            Err(e) => {
                eprintln!("  ✗ {e}");
                return ExitCode::from(1);
            }
        };
        println!(
            "  ✓ LavaTeX installed in {}",
            helper[0].parent().unwrap_or(Path::new("")).display()
        );
        if let Err(code) = start_and_check_helper(&helper, args.port, &token) {
            return code;
        }
        println!("  ✓ Helper is running and paired with the plugin");
    }

    println!(
        "\nDone. Restart ONLYOFFICE, open a presentation, and choose Plugins › LavaTeX.\nTo remove everything again: lavatex setup onlyoffice --uninstall"
    );
    ExitCode::SUCCESS
}

fn uninstall_onlyoffice(args: &OnlyofficeArgs) -> ExitCode {
    println!("Removing LavaTeX from ONLYOFFICE…\n");
    let mut dirs = target_plugin_dirs(args);
    if args.plugins_dir.is_none() {
        // Also clean up locations whose ONLYOFFICE folder is gone.
        if let Some(home) = home() {
            for (label, _, plugins) in onlyoffice_locations(&home) {
                if !dirs.iter().any(|(_, d)| d == &plugins) {
                    dirs.push((label.to_string(), plugins));
                }
            }
        }
    }
    for (label, dir) in dirs {
        let plugin = dir.join(PLUGIN_GUID);
        if plugin.exists() {
            match std::fs::remove_dir_all(&plugin) {
                Ok(()) => println!("  ✓ Plugin removed from {label}"),
                Err(e) => eprintln!("  ✗ Cannot remove {}: {e}", plugin.display()),
            }
        }
    }
    if powerpoint_installed() {
        println!("  - The helper stays installed: the PowerPoint add-in uses it.");
    } else {
        remove_shared_install(args.port);
    }
    println!(
        "\nDone. Your settings (TeX choice, access token) are kept in the LavaTeX config folder."
    );
    ExitCode::SUCCESS
}

/// Whether the ONLYOFFICE plugin is installed for this user.
fn onlyoffice_installed() -> bool {
    home().is_some_and(|h| {
        onlyoffice_locations(&h)
            .iter()
            .any(|(_, _, plugins)| plugins.join(PLUGIN_GUID).exists())
    })
}

/// Whether the PowerPoint add-in is set up for this user.
fn powerpoint_installed() -> bool {
    install_dir().is_some_and(|d| d.join(ADDIN_MANIFEST_FILE).exists())
}

/// Stop the helper and remove its autostart entry and installed programs.
fn remove_shared_install(port: u16) {
    for item in unregister_autostart() {
        println!("  ✓ {item}");
    }
    // A helper started without systemd keeps running after its login entry is
    // gone (and on Windows it would keep its program file locked).
    let token = lavatex_core::config::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("helper-token")).ok());
    if let Some(token) = token {
        if http_get(port, "/v1/health", None).is_some() {
            stop_running_helper(port, token.trim());
            println!("  ✓ Helper stopped");
        }
    }
    if let Some(dir) = install_dir().filter(|d| d.exists()) {
        let running = std::env::current_exe()
            .ok()
            .and_then(|p| p.canonicalize().ok());
        for name in ["lavatex", "lavatex-helper"] {
            let path = dir.join(exe_name(name));
            if !path.exists() {
                continue;
            }
            if cfg!(windows) && path.canonicalize().ok() == running {
                println!(
                    "  - {} is in use; delete it yourself after this finishes.",
                    path.display()
                );
                continue;
            }
            let _ = std::fs::remove_file(&path);
        }
        let _ = std::fs::remove_dir(&dir);
        println!("  ✓ Installed programs removed");
    }
}

/// Where Office for Mac picks up sideloaded add-in manifests.
fn mac_wef_dir() -> Option<PathBuf> {
    home().map(|h| h.join("Library/Containers/com.microsoft.Powerpoint/Data/Documents/wef"))
}

fn nss_db() -> Option<String> {
    home().map(|h| format!("sql:{}", h.join(".pki/nssdb").display()))
}

/// Add the CA to the user's trust store. Returns a sentence describing it.
fn trust_ca(ca: &Path, replace_old: bool) -> Result<String, String> {
    let ca_str = ca.to_string_lossy().into_owned();
    if cfg!(windows) {
        if replace_old {
            // Windows asks before deleting; an old certificate left behind is harmless.
            let _ = run_quiet(
                "certutil",
                &["-user", "-delstore", "Root", tls::CA_COMMON_NAME],
            );
        }
        run_quiet("certutil", &["-user", "-addstore", "Root", &ca_str]).map_err(|e| {
            format!("{e}\n    Windows asks you to confirm the certificate; click Yes, then run this again.")
        })?;
        return Ok("trusted for your Windows account".into());
    }
    if cfg!(target_os = "macos") {
        let keychain = home()
            .ok_or("no home folder")?
            .join("Library/Keychains/login.keychain-db")
            .to_string_lossy()
            .into_owned();
        if replace_old {
            while run_quiet(
                "security",
                &["delete-certificate", "-c", tls::CA_COMMON_NAME, &keychain],
            )
            .is_ok()
            {}
        }
        run_quiet(
            "security",
            &[
                "add-trusted-cert",
                "-r",
                "trustRoot",
                "-k",
                &keychain,
                &ca_str,
            ],
        )
        .map_err(|e| format!("{e}\n    macOS asks for your password to trust the certificate."))?;
        return Ok("trusted in your login keychain".into());
    }
    // Linux: PowerPoint for the web in Chrome or Chromium (NSS database).
    let db = nss_db().ok_or("no home folder")?;
    if Command::new("certutil").arg("-H").output().is_err() {
        return Err(format!(
            "to use PowerPoint for the web in a browser, trust {ca_str} in the browser's certificate settings (Chrome: Settings › Privacy and security › Security › Manage certificates)"
        ));
    }
    let _ = std::fs::create_dir_all(home().unwrap_or_default().join(".pki/nssdb"));
    let _ = run_quiet("certutil", &["-d", &db, "-D", "-n", tls::CA_COMMON_NAME]);
    run_quiet(
        "certutil",
        &[
            "-d",
            &db,
            "-A",
            "-t",
            "C,,",
            "-n",
            tls::CA_COMMON_NAME,
            "-i",
            &ca_str,
        ],
    )?;
    Ok("trusted for Chrome and Chromium (NSS)".into())
}

fn untrust_ca() -> Option<String> {
    if cfg!(windows) {
        return run_quiet(
            "certutil",
            &["-user", "-delstore", "Root", tls::CA_COMMON_NAME],
        )
        .ok()
        .map(|_| "Certificate removed from your Windows account".into());
    }
    if cfg!(target_os = "macos") {
        let keychain = home()?.join("Library/Keychains/login.keychain-db");
        let keychain = keychain.to_string_lossy();
        let mut removed = false;
        while run_quiet(
            "security",
            &["delete-certificate", "-c", tls::CA_COMMON_NAME, &keychain],
        )
        .is_ok()
        {
            removed = true;
        }
        return removed.then(|| "Certificate removed from your login keychain".into());
    }
    let db = nss_db()?;
    run_quiet("certutil", &["-d", &db, "-D", "-n", tls::CA_COMMON_NAME])
        .ok()
        .map(|_| "Certificate removed from Chrome/Chromium".into())
}

/// Make PowerPoint load the add-in. Returns a sentence describing it.
fn register_addin(manifest: &Path) -> Result<String, String> {
    if cfg!(windows) {
        run_quiet(
            "reg",
            &[
                "add",
                WEF_DEVELOPER_KEY,
                "/v",
                ADDIN_ID,
                "/t",
                "REG_SZ",
                "/d",
                &manifest.to_string_lossy(),
                "/f",
            ],
        )?;
        return Ok("registered with PowerPoint".into());
    }
    if cfg!(target_os = "macos") {
        let dir = mac_wef_dir().ok_or("no home folder")?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        std::fs::copy(manifest, dir.join(ADDIN_MANIFEST_FILE)).map_err(|e| e.to_string())?;
        return Ok("registered with PowerPoint for Mac".into());
    }
    Ok("ready for PowerPoint for the web: Home › Add-ins › More Add-ins › My Add-ins › Upload My Add-in, then choose this file".into())
}

fn unregister_addin() -> Option<String> {
    if cfg!(windows) {
        return run_quiet("reg", &["delete", WEF_DEVELOPER_KEY, "/v", ADDIN_ID, "/f"])
            .ok()
            .map(|_| "Add-in unregistered from PowerPoint".into());
    }
    if cfg!(target_os = "macos") {
        let file = mac_wef_dir()?.join(ADDIN_MANIFEST_FILE);
        return std::fs::remove_file(file)
            .ok()
            .map(|_| "Add-in unregistered from PowerPoint".into());
    }
    None
}

fn setup_powerpoint(args: &PowerpointArgs) -> ExitCode {
    println!("Setting up LavaTeX for PowerPoint…\n");
    report_tex();
    let fail = |msg: String| {
        eprintln!("  ✗ {msg}");
        ExitCode::from(1)
    };

    let token = match load_or_create_token(crate::new_token) {
        Ok(t) => t,
        Err(e) => return fail(format!("Cannot create the access token: {e}")),
    };
    let helper = match install_binaries() {
        Ok(cmd) => cmd,
        Err(e) => return fail(e),
    };
    let bin_dir = helper[0].parent().unwrap_or(Path::new("")).to_path_buf();
    println!("  ✓ LavaTeX installed in {}", bin_dir.display());

    // Office only loads add-ins over HTTPS: the helper gets a certificate
    // that is valid for this computer (localhost) only.
    let Some(tls_dir) = tls::tls_dir() else {
        return fail("cannot find the LavaTeX config folder".into());
    };
    let renew = args.new_certificate
        || !tls::is_configured(&tls_dir)
        || tls::server_config(&tls_dir).is_err();
    if renew {
        if let Err(e) = tls::generate(&tls_dir) {
            return fail(e);
        }
        println!("  ✓ HTTPS certificate created (valid for this computer only)");
    } else {
        println!("  ✓ HTTPS certificate found");
    }
    match trust_ca(&tls_dir.join(tls::CA_FILE), renew) {
        Ok(how) => println!("  ✓ Certificate {how}"),
        Err(e) if cfg!(any(windows, target_os = "macos")) => {
            return fail(format!("The certificate could not be trusted: {e}"))
        }
        Err(e) => println!("  - Certificate not trusted automatically: {e}"),
    }

    let manifest = bin_dir.join(ADDIN_MANIFEST_FILE);
    if let Err(e) = std::fs::write(
        &manifest,
        crate::serve::addin_manifest(https_port(args.port)),
    ) {
        return fail(format!("cannot write {}: {e}", manifest.display()));
    }
    match register_addin(&manifest) {
        Ok(how) => println!("  ✓ Add-in {how}\n      {}", manifest.display()),
        Err(e) => return fail(format!("Cannot register the add-in: {e}")),
    }

    if args.no_autostart {
        println!(
            "  - Automatic start skipped. Run \"lavatex serve\" yourself when you use PowerPoint."
        );
    } else {
        // Restarting the helper also makes it pick up the certificate.
        if let Err(code) = start_and_check_helper(&helper, args.port, &token) {
            return code;
        }
        let https = https_port(args.port);
        let mut served = false;
        for _ in 0..20 {
            if let Ok((200, body)) = tls::https_get(&tls_dir, https, "/powerpoint/manifest.xml") {
                served = body.contains(ADDIN_ID);
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        if !served {
            return fail(format!(
                "The helper does not serve the add-in on https://localhost:{https}. Is another program using that port?"
            ));
        }
        println!("  ✓ Add-in is served at https://localhost:{https}");
    }

    if cfg!(any(windows, target_os = "macos")) {
        println!("\nDone. Restart PowerPoint: the LavaTeX tab appears on the ribbon.");
    } else {
        println!("\nDone. In PowerPoint for the web, upload the add-in file shown above; the LavaTeX tab then appears on the ribbon.");
    }
    println!("To remove everything again: lavatex setup powerpoint --uninstall");
    ExitCode::SUCCESS
}

fn uninstall_powerpoint(args: &PowerpointArgs) -> ExitCode {
    println!("Removing LavaTeX from PowerPoint…\n");
    if let Some(done) = unregister_addin() {
        println!("  ✓ {done}");
    }
    if let Some(manifest) = install_dir().map(|d| d.join(ADDIN_MANIFEST_FILE)) {
        let _ = std::fs::remove_file(manifest);
    }
    if let Some(done) = untrust_ca() {
        println!("  ✓ {done}");
    }
    if let Some(dir) = tls::tls_dir().filter(|d| d.exists()) {
        let _ = std::fs::remove_dir_all(dir);
        println!("  ✓ HTTPS certificate deleted");
    }
    if onlyoffice_installed() {
        // Restart the helper without HTTPS; the ONLYOFFICE plugin keeps using it.
        let token = load_or_create_token(crate::new_token).unwrap_or_default();
        let helper = install_dir().map(|d| d.join(exe_name("lavatex-helper")));
        match helper.filter(|h| h.is_file()) {
            Some(h) if register_autostart(std::slice::from_ref(&h), args.port, &token).is_ok() => {
                println!("  - The helper stays installed: the ONLYOFFICE plugin uses it.")
            }
            _ => println!(
                "  - The helper stays installed for the ONLYOFFICE plugin; it stops serving the add-in after you log in again."
            ),
        }
    } else {
        remove_shared_install(args.port);
    }
    println!(
        "\nDone. Your settings (TeX choice, access token) are kept in the LavaTeX config folder."
    );
    ExitCode::SUCCESS
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_is_embedded_without_test_files() {
        let names: Vec<&str> = embedded::PLUGIN_FILES.iter().map(|(n, _)| *n).collect();
        for required in [
            "config.json",
            "index.html",
            "scripts/plugin.js",
            "scripts/sdk-loader.js",
            "scripts/lavatex-core.js",
        ] {
            assert!(
                names.contains(&required),
                "{required} missing from {names:?}"
            );
        }
        assert!(!names.iter().any(|n| n.starts_with("tests/")));
        let config = embedded::PLUGIN_FILES
            .iter()
            .find(|(n, _)| *n == "config.json")
            .unwrap()
            .1;
        let config: serde_json::Value = serde_json::from_slice(config).unwrap();
        assert_eq!(config["guid"], format!("asc.{PLUGIN_GUID}"));
    }

    #[test]
    fn installs_plugin_with_pairing_file() {
        let dir = tempfile::tempdir().unwrap();
        let installed = install_plugin(dir.path(), 7354, "secret-token-0123456789").unwrap();
        assert!(installed.ends_with(PLUGIN_GUID));
        assert!(installed.join("index.html").is_file());
        let pairing: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(installed.join(PAIRING_FILE)).unwrap())
                .unwrap();
        assert_eq!(pairing["url"], "http://127.0.0.1:7354");
        assert_eq!(pairing["token"], "secret-token-0123456789");
        // Reinstalling replaces old files.
        std::fs::write(installed.join("stale.txt"), "x").unwrap();
        install_plugin(dir.path(), 7354, "secret-token-0123456789").unwrap();
        assert!(!installed.join("stale.txt").exists());
    }

    #[test]
    fn systemd_units_use_socket_activation() {
        let (socket, service) = systemd_units(
            &[PathBuf::from(
                "/home/me/.local/share/lavatex/bin/lavatex-helper",
            )],
            7354,
            false,
            "/usr/bin:/opt/100%",
        );
        assert!(socket.contains("ListenStream=127.0.0.1:7354"));
        assert!(!socket.contains("7355"));
        let (socket, _) = systemd_units(&[PathBuf::from("/x/lavatex-helper")], 7354, true, "");
        assert!(socket.contains("ListenStream=127.0.0.1:7354\nListenStream=127.0.0.1:7355\n"));
        assert!(service.contains(
            "ExecStart=/home/me/.local/share/lavatex/bin/lavatex-helper --idle-exit 900"
        ));
        assert!(service.contains("Environment=\"PATH=/usr/bin:/opt/100%%\""));
    }

    #[test]
    fn addin_manifest_matches_embedded_files() {
        let manifest = crate::serve::addin_manifest(7355);
        assert!(!manifest.contains("__LAVATEX_"), "unfilled placeholder");
        assert!(manifest.contains(&format!("<Id>{ADDIN_ID}</Id>")));
        let version = format!("<Version>{}.0</Version>", lavatex_core::VERSION);
        assert!(manifest.contains(&version), "expected {version}");
        // Every add-in URL in the manifest must be a file the helper serves.
        let prefix = "https://localhost:7355/powerpoint/";
        let mut checked = 0;
        for part in manifest.split(prefix).skip(1) {
            let rel = part.split(['"', '?']).next().unwrap();
            assert!(
                embedded::ADDIN_FILES.iter().any(|(n, _)| *n == rel),
                "manifest refers to {rel}, which is not embedded"
            );
            checked += 1;
        }
        assert!(checked > 20, "only {checked} URLs found");
        for page in ["taskpane.html", "commands.html", "dialog.html"] {
            let (_, bytes) = embedded::ADDIN_FILES
                .iter()
                .find(|(n, _)| *n == page)
                .unwrap();
            let text = String::from_utf8_lossy(bytes);
            assert!(text.contains("appsforoffice.microsoft.com/lib/1/hosted/office.js"));
        }
        assert!(!embedded::ADDIN_FILES
            .iter()
            .any(|(n, _)| n.starts_with("tests/")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_locations_include_flatpak() {
        let locs = onlyoffice_locations(Path::new("/home/me"));
        assert!(locs.iter().any(|(_, _, p)| p
            == Path::new("/home/me/.var/app/org.onlyoffice.desktopeditors/data/onlyoffice/desktopeditors/data/sdkjs-plugins")));
    }
}
