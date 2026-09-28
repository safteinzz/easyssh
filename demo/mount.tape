# Mount a remote folder: the form with the sshfs line it will run, the mount
# on the Mounts tab, raspi's home listed from the shell like any local folder,
# then Enter taking it down and bringing it back, the line kept either way.
#
#   cd demo && vhs mount.tape && ./stage.sh down
#
# `raspi` is the stage's throwaway sshd, so the mount is real.

Output "../readme-assets/mount.gif"

Set Shell bash
Set FontFamily "JetBrainsMono NF"
Set Theme "Catppuccin Mocha"
Set FontSize 15
Set Width 1240
Set Height 560
Set Padding 20
Set Framerate 24
Set TypingSpeed 60ms

Hide
Type "./stage.sh up >/dev/null 2>&1"
Enter
Sleep 4s
# `exec`, so a stage that refuses to open takes the terminal with it: without it
# the script exits, the outer shell survives, and every keystroke after this
# lands in the *real* home - which is how a README image ends up showing
# somebody's actual connections.
Type "exec ./stage.sh shell"
Enter
Sleep 2s
# The first frame is what the README shows before the GIF plays, so it opens on
# the toolbox, not on a shell prompt.
Type "essh"
Enter
Sleep 3s
Show
Sleep 2s

Type "/"
Sleep 300ms
Type "raspi"
Sleep 800ms
Enter
Sleep 800ms

# ---- the form, and the command it builds ---------------------------------
Type "m"
Sleep 3s
Enter
Sleep 400ms
Enter
Sleep 400ms
Enter
Sleep 3s

# ---- it is just a folder -------------------------------------------------
Type "q"
Sleep 800ms
Type "ls ~/sshfs/raspi"
Enter
Sleep 3s
Type "essh"
Enter
Sleep 2s
Type "l"
Sleep 300ms
Type "l"
Sleep 300ms
Type "l"
Sleep 1.5s

# ---- Enter is the off switch, and the on switch --------------------------
Enter
Sleep 2.5s
Enter
Sleep 3s

# Off again before quitting, so the stage is torn down with nothing mounted.
Hide
Enter
Sleep 1.5s
Type "q"
Sleep 1s
