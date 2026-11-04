# Tool smoke check only; not language correctness or release acceptance.
Set Size 90x10
Set TypingDelay 20ms
Set Title "IntentumDiff · recorder smoke check"
Set Caption "Checks the recorder against the selected installed CLI"
Type "intentumdiff --version"
Enter
Wait /[Ii]ntentum[Dd]iff [0-9]/
Sleep 500ms
Screenshot version
