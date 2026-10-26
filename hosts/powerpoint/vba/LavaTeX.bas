Attribute VB_Name = "LavaTeX"
' LavaTeX for PowerPoint (LavaTeX.ppam).
'
' The ribbon buttons call the LavaTeX_* procedures below. Rendering and the
' editor window are done by lavatex-helper.exe, which is looked for next to
' the .ppam file and in %LOCALAPPDATA%\Programs\LavaTeX. It writes the
' picture, alt.txt and result.txt into a temporary folder; this module puts
' the picture on the slide.
'
' The LaTeX source travels with the picture in its alt text and in the shape
' tag LAVATEX, in the same format as LibreOffice and ONLYOFFICE use.
'
' Only plain ASCII in this file: the VBA project is stored in code page 1252.
Option Explicit

Private Const LAVATEX_VERSION As String = "0.2.0"
Private Const ENGINE_EXE As String = "lavatex-helper.exe"
Private Const TAG_NAME As String = "LAVATEX"
Private Const MARKER As String = "lavatex:1:"
Private Const SHAPE_NAME As String = "LavaTeX equation"
Private Const HELP_URL As String = "https://github.com/LavakumarVeludandi/LavaTex/blob/master/docs/usage.md"

' Exit codes of lavatex-helper.exe.
Private Const EXIT_OK As Long = 0
Private Const EXIT_CANCELLED As Long = 1

' PowerPoint and Office constants, as numbers (no enum references needed).
Private Const SEL_SHAPES As Long = 2       ' ppSelectionShapes
Private Const SEL_TEXT As Long = 3         ' ppSelectionText
Private Const MSO_FALSE As Long = 0
Private Const MSO_TRUE As Long = -1
Private Const SEND_BACKWARD As Long = 3    ' msoSendBackward

#If Mac Then
#ElseIf VBA7 Then
Private Declare PtrSafe Sub Sleep Lib "kernel32" (ByVal dwMilliseconds As Long)
#Else
Private Declare Sub Sleep Lib "kernel32" (ByVal dwMilliseconds As Long)
#End If

' ---------------------------------------------------------------------------
' Ribbon buttons

Public Sub LavaTeX_New(control As Object)
    RunEditor False
End Sub

Public Sub LavaTeX_Edit(control As Object)
    RunEditor True
End Sub

Public Sub LavaTeX_Regenerate(control As Object)
    RegenerateSelection
End Sub

Public Sub LavaTeX_Help(control As Object)
    On Error Resume Next
    CreateObject("Shell.Application").ShellExecute HELP_URL
End Sub

Public Sub LavaTeX_About(control As Object)
    MsgBox "LavaTeX " & LAVATEX_VERSION & vbCrLf & vbCrLf & _
        "LaTeX equations as editable pictures, rendered with the TeX installed on this computer." & vbCrLf & _
        "https://github.com/LavakumarVeludandi/LavaTex", vbInformation, "LavaTeX"
End Sub

' ---------------------------------------------------------------------------
' New / edit

Private Sub RunEditor(ByVal wantEdit As Boolean)
    #If Mac Then
        MsgBox "LavaTeX for PowerPoint currently works on Windows only.", vbInformation, "LavaTeX"
        Exit Sub
    #End If
    On Error GoTo Failed

    Dim sld As Object
    Set sld = CurrentSlide()
    If sld Is Nothing Then
        MsgBox "Open a slide in Normal view first.", vbInformation, "LavaTeX"
        Exit Sub
    End If

    Dim target As Object, altText As String
    Set target = SelectedEquation(altText)
    If wantEdit And target Is Nothing Then
        MsgBox "Select a LavaTeX equation on the slide, then click Edit Equation." & vbCrLf & _
            "(To type a new one, click New Equation.)", vbInformation, "LavaTeX"
        Exit Sub
    End If
    If Not wantEdit Then Set target = Nothing

    Dim engine As String
    engine = EnginePath()
    If engine = "" Then Exit Sub

    Dim work As String
    work = NewWorkFolder()
    Dim args As String
    args = "editor --out " & Quote(work & "\out")
    If Not target Is Nothing Then
        WriteUtf8 work & "\in.txt", altText
        args = args & " --alt-text-file " & Quote(work & "\in.txt")
    End If

    Dim code As Long
    code = RunEngine(engine, args)
    If code = EXIT_CANCELLED Then GoTo Done
    If code <> EXIT_OK Then
        ShowEngineError work & "\out", code
        GoTo Done
    End If
    PlaceResult sld, work & "\out", target, engine
Done:
    DeleteFolder work
    Exit Sub
Failed:
    MsgBox "LavaTeX ran into a problem: " & Err.Description, vbExclamation, "LavaTeX"
End Sub

' ---------------------------------------------------------------------------
' Regenerate: render the selected equations again from their LaTeX

Private Sub RegenerateSelection()
    #If Mac Then
        MsgBox "LavaTeX for PowerPoint currently works on Windows only.", vbInformation, "LavaTeX"
        Exit Sub
    #End If
    On Error GoTo Failed

    Dim sld As Object
    Set sld = CurrentSlide()
    Dim shapes As New Collection, alts As New Collection
    If Not sld Is Nothing Then
        If ActiveWindow.Selection.Type = SEL_SHAPES Or ActiveWindow.Selection.Type = SEL_TEXT Then
            Dim shp As Object, a As String
            For Each shp In ActiveWindow.Selection.ShapeRange
                a = EquationText(shp)
                If a <> "" Then
                    shapes.Add shp
                    alts.Add a
                End If
            Next shp
        End If
    End If
    If shapes.Count = 0 Then
        MsgBox "Select one or more LavaTeX equations, then click Regenerate.", vbInformation, "LavaTeX"
        Exit Sub
    End If

    Dim engine As String
    engine = EnginePath()
    If engine = "" Then Exit Sub

    Dim i As Long, work As String, code As Long, problems As String, done As Long
    For i = 1 To shapes.Count
        work = NewWorkFolder()
        WriteUtf8 work & "\in.txt", alts(i)
        code = RunEngine(engine, "rerender --alt-text-file " & Quote(work & "\in.txt") & " --out " & Quote(work & "\out"))
        If code = EXIT_OK Then
            PlaceResult sld, work & "\out", shapes(i), engine
            done = done + 1
        Else
            problems = problems & "- " & FirstLine(alts(i)) & ": " & ReadErrorText(work & "\out") & vbCrLf
        End If
        DeleteFolder work
    Next i
    If problems <> "" Then
        MsgBox done & " of " & shapes.Count & " equation(s) regenerated. These could not be:" & vbCrLf & vbCrLf & problems, _
            vbExclamation, "LavaTeX"
    End If
    Exit Sub
Failed:
    MsgBox "LavaTeX ran into a problem: " & Err.Description, vbExclamation, "LavaTeX"
End Sub

' ---------------------------------------------------------------------------
' Placing pictures

' Put the rendered picture on the slide. With `old`, replace that shape and
' keep its scale, position, rotation and place in the stacking order.
Private Sub PlaceResult(ByVal sld As Object, ByVal outDir As String, ByVal old As Object, ByVal engine As String)
    Dim result As Object
    Set result = ReadResult(outDir & "\result.txt")
    Dim pic As String, altText As String
    pic = outDir & "\" & result("file")
    altText = ReadUtf8(outDir & "\alt.txt")

    Dim w As Double, h As Double, scaleBy As Double
    w = ToNumber(result("widthBp"))
    h = ToNumber(result("heightBp"))
    scaleBy = 1
    If Not old Is Nothing And result.Exists("previousWidthBp") Then
        If ToNumber(result("previousWidthBp")) > 0 Then
            scaleBy = old.Width / ToNumber(result("previousWidthBp"))
            If scaleBy < 0.01 Or scaleBy > 100 Then scaleBy = 1
        End If
    End If
    w = w * scaleBy
    h = h * scaleBy

    Dim l As Double, t As Double
    If old Is Nothing Then
        l = (ActivePresentation.PageSetup.SlideWidth - w) / 2
        t = (ActivePresentation.PageSetup.SlideHeight - h) / 2
    Else
        l = old.Left
        t = old.Top
    End If

    Dim shp As Object
    Set shp = AddPicture(sld, pic, l, t, w, h)
    If shp Is Nothing And LCase(result("format")) = "svg" Then
        ' PowerPoint versions without SVG support: use a PNG instead.
        Dim work As String
        work = NewWorkFolder()
        WriteUtf8 work & "\in.txt", altText
        If RunEngine(engine, "rerender --format png --alt-text-file " & Quote(work & "\in.txt") & " --out " & Quote(work & "\out")) = EXIT_OK Then
            Set shp = AddPicture(sld, work & "\out\equation.png", l, t, w, h)
        End If
        DeleteFolder work
    End If
    If shp Is Nothing Then
        MsgBox "PowerPoint could not insert the picture.", vbExclamation, "LavaTeX"
        Exit Sub
    End If

    shp.Name = SHAPE_NAME
    shp.AlternativeText = altText
    shp.Tags.Add TAG_NAME, altText
    On Error Resume Next
    shp.Title = "LaTeX equation"
    On Error GoTo 0

    If Not old Is Nothing Then
        shp.Rotation = old.Rotation
        Dim guard As Long
        Do While shp.ZOrderPosition > old.ZOrderPosition + 1 And guard < 1000
            shp.ZOrder SEND_BACKWARD
            guard = guard + 1
        Loop
        old.Delete
    End If
    shp.Select
End Sub

Private Function AddPicture(ByVal sld As Object, ByVal path As String, ByVal l As Double, ByVal t As Double, ByVal w As Double, ByVal h As Double) As Object
    On Error Resume Next
    Set AddPicture = sld.Shapes.AddPicture(path, MSO_FALSE, MSO_TRUE, l, t, w, h)
    If Err.Number <> 0 Then Set AddPicture = Nothing
    On Error GoTo 0
End Function

' ---------------------------------------------------------------------------
' Selection

Private Function CurrentSlide() As Object
    On Error Resume Next
    Set CurrentSlide = ActiveWindow.View.Slide
    If Err.Number <> 0 Then Set CurrentSlide = Nothing
    On Error GoTo 0
End Function

' The selected shape if it is exactly one LavaTeX equation; its record text
' (tag or alt text) goes into `altText`.
Private Function SelectedEquation(ByRef altText As String) As Object
    Set SelectedEquation = Nothing
    altText = ""
    On Error GoTo NoSelection
    If ActiveWindow.Selection.Type <> SEL_SHAPES Then Exit Function
    If ActiveWindow.Selection.ShapeRange.Count <> 1 Then Exit Function
    Dim shp As Object
    Set shp = ActiveWindow.Selection.ShapeRange(1)
    altText = EquationText(shp)
    If altText <> "" Then Set SelectedEquation = shp
    Exit Function
NoSelection:
    Set SelectedEquation = Nothing
End Function

' The LavaTeX record text of a shape ("" if it is not an equation).
Private Function EquationText(ByVal shp As Object) As String
    Dim s As String
    On Error Resume Next
    s = shp.Tags(TAG_NAME)
    If InStr(1, s, MARKER, vbBinaryCompare) = 0 Then s = shp.AlternativeText
    On Error GoTo 0
    If InStr(1, s, MARKER, vbBinaryCompare) > 0 Then EquationText = s Else EquationText = ""
End Function

' ---------------------------------------------------------------------------
' The engine (lavatex-helper.exe, installed with LavaTeX)

Private Function EnginePath() As String
    Dim candidates(1 To 2) As String, i As Long, addinFile As String
    addinFile = AddinPath()
    If addinFile <> "" Then candidates(1) = Left$(addinFile, InStrRev(addinFile, "\")) & ENGINE_EXE
    candidates(2) = Environ$("LOCALAPPDATA") & "\Programs\LavaTeX\" & ENGINE_EXE
    For i = 1 To 2
        If candidates(i) <> "" Then
            If Len(Dir$(candidates(i))) > 0 Then
                EnginePath = candidates(i)
                Exit Function
            End If
        End If
    Next i
    EnginePath = ""
    MsgBox "LavaTeX cannot find " & ENGINE_EXE & "." & vbCrLf & vbCrLf & _
        "Put it in the same folder as LavaTeX.ppam, or in " & Environ$("LOCALAPPDATA") & "\Programs\LavaTeX.", _
        vbExclamation, "LavaTeX"
End Function

' Full path of this add-in's .ppam file.
Private Function AddinPath() As String
    Dim a As Object
    AddinPath = ""
    On Error Resume Next
    For Each a In Application.AddIns
        If LCase$(a.Name) Like "lavatex*" Then
            AddinPath = a.FullName
            If Len(Dir$(AddinPath)) > 0 Then Exit Function
        End If
    Next a
    AddinPath = ""
End Function

' Run the engine and wait for it, keeping PowerPoint responsive. Returns its
' exit code.
Private Function RunEngine(ByVal exe As String, ByVal args As String) As Long
    Dim sh As Object, proc As Object
    Set sh = CreateObject("WScript.Shell")
    Set proc = sh.Exec(Quote(exe) & " " & args)
    Do While proc.Status = 0
        DoEvents
        #If Not Mac Then
            Sleep 30
        #End If
    Loop
    RunEngine = proc.ExitCode
End Function

Private Sub ShowEngineError(ByVal outDir As String, ByVal code As Long)
    MsgBox ReadErrorText(outDir) & vbCrLf & vbCrLf & "(lavatex-helper exit code " & code & ")", vbExclamation, "LavaTeX"
End Sub

Private Function ReadErrorText(ByVal outDir As String) As String
    If Len(Dir$(outDir & "\error.txt")) > 0 Then
        ReadErrorText = ReadUtf8(outDir & "\error.txt")
    Else
        ReadErrorText = "The LavaTeX engine stopped unexpectedly."
    End If
End Function

' ---------------------------------------------------------------------------
' Files

Private Function NewWorkFolder() As String
    Dim fso As Object, base As String, path As String
    Set fso = CreateObject("Scripting.FileSystemObject")
    base = Environ$("TEMP") & "\LavaTeX"
    MakeFolders base
    Randomize
    Do
        path = base & "\" & Format$(Now, "yyyymmddhhnnss") & "-" & CStr(Int(Rnd() * 1000000))
    Loop While fso.FolderExists(path)
    fso.CreateFolder path
    NewWorkFolder = path
End Function

' Create `path` and any missing parent folders. Uses FileSystemObject:
' Dir$() does not see hidden folders such as AppData.
Private Sub MakeFolders(ByVal path As String)
    Dim fso As Object
    Set fso = CreateObject("Scripting.FileSystemObject")
    If path = "" Or fso.FolderExists(path) Then Exit Sub
    MakeFolders fso.GetParentFolderName(path)
    fso.CreateFolder path
End Sub

Private Sub DeleteFolder(ByVal path As String)
    On Error Resume Next
    CreateObject("Scripting.FileSystemObject").DeleteFolder path, True
End Sub

Private Function Quote(ByVal s As String) As String
    Quote = """" & s & """"
End Function

Private Sub WriteUtf8(ByVal path As String, ByVal text As String)
    Dim st As Object
    Set st = CreateObject("ADODB.Stream")
    st.Type = 2 ' text
    st.Charset = "utf-8"
    st.Open
    st.WriteText text
    st.SaveToFile path, 2 ' overwrite
    st.Close
End Sub

Private Function ReadUtf8(ByVal path As String) As String
    Dim st As Object
    Set st = CreateObject("ADODB.Stream")
    st.Type = 2
    st.Charset = "utf-8"
    st.Open
    st.LoadFromFile path
    ReadUtf8 = st.ReadText
    st.Close
End Function

' result.txt: key=value lines.
Private Function ReadResult(ByVal path As String) As Object
    Dim d As Object, lines() As String, i As Long, pos As Long
    Set d = CreateObject("Scripting.Dictionary")
    lines = Split(Replace(ReadUtf8(path), vbCr, ""), vbLf)
    For i = 0 To UBound(lines)
        pos = InStr(lines(i), "=")
        If pos > 1 Then d(Left$(lines(i), pos - 1)) = Mid$(lines(i), pos + 1)
    Next i
    Set ReadResult = d
End Function

' Numbers in result.txt use a dot, whatever the Windows locale says.
Private Function ToNumber(ByVal s As String) As Double
    ToNumber = Val(s)
End Function

Private Function FirstLine(ByVal s As String) As String
    Dim p As Long
    p = InStr(s, vbLf)
    If p > 0 Then s = Left$(s, p - 1)
    s = Replace(s, vbCr, "")
    If Len(s) > 40 Then s = Left$(s, 40) & "..."
    FirstLine = s
End Function
