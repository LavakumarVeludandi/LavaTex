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
' Equations work both ways with IguanaTeX: LavaTeX edits IguanaTeX displays
' (LaTeX in the tag LATEXADDIN, size in IGUANATEXSIZE), and writes those tags
' on its own pictures too, so IguanaTeX can edit them. Whichever add-in
' edits last makes a new shape with its own record.
'
' Only plain ASCII in this file: the VBA project is stored in code page 1252.
Option Explicit

Private Const LAVATEX_VERSION As String = "0.2.0"
Private Const ENGINE_EXE As String = "lavatex-helper.exe"
Private Const TAG_NAME As String = "LAVATEX"
Private Const MARKER As String = "lavatex:1:"
Private Const IGUANA_TAG As String = "LATEXADDIN"
Private Const IGUANA_SIZE_TAG As String = "IGUANATEXSIZE"
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
Private Const MSO_GROUP As Long = 6        ' msoGroup

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

    Dim target As Object, altText As String, iguanaSize As String
    Set target = SelectedEquation(altText, iguanaSize)
    If wantEdit And target Is Nothing Then
        MsgBox "Select a LavaTeX or IguanaTeX equation on the slide, then click Edit Equation." & vbCrLf & _
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
    If Not target Is Nothing Then args = args & InputArgs(work, altText, iguanaSize)

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
    Dim shapes As New Collection, alts As New Collection, sizes As New Collection
    If Not sld Is Nothing Then
        Dim shp As Object, a As String, sz As String
        For Each shp In SelectedShapes()
            a = EquationText(shp, sz)
            If a <> "" Then
                shapes.Add shp
                alts.Add a
                sizes.Add sz
            End If
        Next shp
    End If
    If shapes.Count = 0 Then
        MsgBox "Select one or more LavaTeX or IguanaTeX equations, then click Regenerate.", vbInformation, "LavaTeX"
        Exit Sub
    End If

    Dim engine As String
    engine = EnginePath()
    If engine = "" Then Exit Sub

    Dim i As Long, work As String, code As Long, problems As String, done As Long
    For i = 1 To shapes.Count
        work = NewWorkFolder()
        code = RunEngine(engine, "rerender" & InputArgs(work, alts(i), sizes(i)) & " --out " & Quote(work & "\out"))
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
' keep its scale, position, rotation, group and place in the stacking order.
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
    If Not old Is Nothing Then
        ' The old equation's width before anyone resized it.
        Dim natural As Double
        If result.Exists("previousWidthBp") Then natural = ToNumber(result("previousWidthBp")) Else natural = IguanaWidth(old)
        If natural > 0 Then
            scaleBy = old.Width / natural
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
    AddIguanaTeXTags shp, outDir, result
    On Error Resume Next
    shp.Title = "LaTeX equation"
    On Error GoTo 0

    If Not old Is Nothing Then
        shp.Rotation = old.Rotation
        If old.Child = MSO_TRUE Then
            ReplaceInGroup old, shp
        Else
            StackAbove shp, old
            old.Delete
        End If
    End If
    On Error Resume Next ' after regrouping, selecting a group member can fail
    shp.Select
End Sub

' The tags IguanaTeX reads, so it can edit this equation too: LATEXADDIN from
' iguanatex.txt, the others from the `tag.NAME=value` lines of result.txt.
Private Sub AddIguanaTeXTags(ByVal shp As Object, ByVal outDir As String, ByVal result As Object)
    If Not CreateObject("Scripting.FileSystemObject").FileExists(outDir & "\iguanatex.txt") Then Exit Sub
    shp.Tags.Add IGUANA_TAG, ReadUtf8(outDir & "\iguanatex.txt")
    Dim k As Variant
    For Each k In result.Keys
        If Left$(k, 4) = "tag." Then shp.Tags.Add Mid$(k, 5), result(k)
    Next k
End Sub

' The width IguanaTeX gave a display before anyone resized it, 0 if unknown:
' its 10 pt width (ORIGINALWIDTH) at its point size, for bitmaps also times
' 96 / dpi, the way IguanaTeX sizes them.
Private Function IguanaWidth(ByVal shp As Object) As Double
    Dim w As Double, kind As String
    On Error Resume Next
    w = Val(shp.Tags("ORIGINALWIDTH")) * Val(shp.Tags(IGUANA_SIZE_TAG)) / 10
    kind = shp.Tags("OUTPUTTYPE")
    If kind = "PNG" Or (kind = "" And shp.Tags("BitmapVector") <> "1") Then
        If Val(shp.Tags("OUTPUTDPI")) <= 0 Then Exit Function
        w = w * 96 / Val(shp.Tags("OUTPUTDPI"))
    End If
    IguanaWidth = w
End Function

' Move `shp` just above `ref` in the stacking order.
Private Sub StackAbove(ByVal shp As Object, ByVal ref As Object)
    Dim guard As Long
    Do While shp.ZOrderPosition > ref.ZOrderPosition + 1 And guard < 1000
        shp.ZOrder SEND_BACKWARD
        guard = guard + 1
    Loop
End Sub

' Swap `old`, a member of a group, for `shp`: PowerPoint cannot add a shape
' to a group, so ungroup each level down to `old` and group again.
' shortcut: animations set on the group itself are lost, as when regrouping
' by hand; keep them (like IguanaTeX does) if users miss them.
Private Sub ReplaceInGroup(ByVal old As Object, ByVal shp As Object)
    Dim top As Object
    Set top = old.ParentGroup
    Do While top.Child = MSO_TRUE
        Set top = top.ParentGroup
    Loop
    Regroup top, old.Id, shp
End Sub

' Ungroup `grp`, put `shp` where the member with id `oldId` was, and group
' again under the same name. Returns the new group.
Private Function Regroup(ByVal grp As Object, ByVal oldId As Long, ByVal shp As Object) As Object
    Dim sld As Object, groupName As String, it As Object
    Dim members As New Collection, keep As New Collection
    Set sld = grp.Parent
    groupName = grp.Name
    For Each it In grp.Ungroup
        members.Add it
    Next it
    For Each it In members
        If it.Id = oldId Then
            StackAbove shp, it
            it.Delete
            keep.Add shp
        ElseIf ContainsId(it, oldId) Then
            keep.Add Regroup(it, oldId, shp)
        Else
            keep.Add it
        End If
    Next it
    Dim g As Object
    Set g = ShapesById(sld, keep).Group
    g.Name = groupName
    Set Regroup = g
End Function

Private Function ContainsId(ByVal shp As Object, ByVal id As Long) As Boolean
    Dim s As Object
    If shp.Type <> MSO_GROUP Then Exit Function
    For Each s In shp.GroupItems
        If s.Id = id Or ContainsId(s, id) Then
            ContainsId = True
            Exit Function
        End If
    Next s
End Function

' A ShapeRange of these slide shapes. By index: names on a slide can repeat.
Private Function ShapesById(ByVal sld As Object, ByVal items As Collection) As Object
    Dim idx() As Variant, i As Long, n As Long, it As Object
    ReDim idx(0 To items.Count - 1)
    For i = 1 To sld.Shapes.Count
        For Each it In items
            If sld.Shapes(i).Id = it.Id Then
                idx(n) = i
                n = n + 1
            End If
        Next it
    Next i
    Set ShapesById = sld.Shapes.Range(idx)
End Function

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

' The selected shape if it is exactly one equation; see EquationText for
' `altText` and `iguanaSize`.
Private Function SelectedEquation(ByRef altText As String, ByRef iguanaSize As String) As Object
    Set SelectedEquation = Nothing
    altText = ""
    Dim picked As Collection
    Set picked = SelectedShapes()
    If picked.Count <> 1 Then Exit Function
    altText = EquationText(picked(1), iguanaSize)
    If altText <> "" Then Set SelectedEquation = picked(1)
End Function

' The selected shapes; inside a group, the selected members. A piece of an
' IguanaTeX vector display (tag EMFchild) stands for the whole display.
Private Function SelectedShapes() As Collection
    Dim picked As New Collection, sel As Object, candidates As Object, shp As Object, eq As Object
    Set SelectedShapes = picked
    On Error GoTo Done
    Set sel = ActiveWindow.Selection
    If sel.Type <> SEL_SHAPES And sel.Type <> SEL_TEXT Then Exit Function
    If sel.HasChildShapeRange Then Set candidates = sel.ChildShapeRange Else Set candidates = sel.ShapeRange
    For Each shp In candidates
        Set eq = shp
        If shp.Tags("EMFchild") <> "" Then Set eq = shp.ParentGroup
        On Error Resume Next
        picked.Add eq, CStr(eq.Id) ' keyed: several pieces of one display count once
        On Error GoTo Done
    Next shp
Done:
End Function

' The LavaTeX record text of a shape (tag or alt text). For an IguanaTeX
' display: its LaTeX, with its point size in `iguanaSize`. "" if neither.
Private Function EquationText(ByVal shp As Object, ByRef iguanaSize As String) As String
    Dim s As String
    iguanaSize = ""
    On Error Resume Next
    s = shp.Tags(TAG_NAME)
    If InStr(1, s, MARKER, vbBinaryCompare) = 0 Then s = shp.AlternativeText
    If InStr(1, s, MARKER, vbBinaryCompare) > 0 Then
        EquationText = s
        Exit Function
    End If
    s = ""
    s = shp.Tags(IGUANA_TAG)
    If Trim$(s) <> "" Then
        ' Val and Str$ always use a dot, whatever the Windows locale.
        iguanaSize = Trim$(Str$(Val(shp.Tags(IGUANA_SIZE_TAG))))
        If Val(iguanaSize) <= 0 Then iguanaSize = "20"
        EquationText = s
    Else
        EquationText = ""
    End If
End Function

' Write an equation's text into the work folder; returns the arguments that
' pass it to the engine.
Private Function InputArgs(ByVal work As String, ByVal text As String, ByVal iguanaSize As String) As String
    WriteUtf8 work & "\in.txt", text
    InputArgs = " --alt-text-file " & Quote(work & "\in.txt")
    If iguanaSize <> "" Then InputArgs = InputArgs & " --iguanatex-size " & iguanaSize
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
