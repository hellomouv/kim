#Requires AutoHotkey v2.0
; Constructs Ddalgak never approximates: each is reported for manual migration with its line.
#Include lib.ahk

counter := 0

#c::Run "calc.exe"
a & b::Send "x"
^!d::DllCall("user32\MessageBeep", "UInt", 0)
^!f::FileAppend "log", "C:\log.txt"
^!r::Run "mytool.exe"
^!h::Send "{Shift down}a{Shift up}"
:X:;now::Send FormatTime()

#HotIf WinActive("Untitled - Notepad")
^!q::Send "q"
#HotIf

MyFunction() {
    MsgBox "hello"
}
