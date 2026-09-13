# A new machine, start to finish: the hosts already here, raspi added with the
# form, its key installed with ssh-copy-id and the password typed that once, then
# Enter logging in on the key alone.
#
#   cd demo && vhs connect.tape && ./stage.sh down
#
# It stages `fresh`: raspi is a throwaway sshd that is not in the config yet
# and takes the password `raspberry` until it has a key.

Output "../readme-assets/connect.gif"

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
Type "./stage.sh fresh >/dev/null 2>&1"
Enter
Sleep 6s
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

# ---- the hosts already here ----------------------------------------------
Down
Sleep 900ms
Down
Sleep 900ms
Up
Sleep 300ms
Up
Sleep 800ms

# ---- a new machine: the form, and ctrl-o for the key ---------------------
Type "c"
Sleep 800ms
Type "raspi"
Enter
Type "127.0.0.6"
Enter
Type "pi"
Enter
Type "2222"
Enter
Sleep 300ms
Ctrl+O
Sleep 1.2s
Enter
Sleep 1.2s
Enter
Enter
Enter
Sleep 2s

# ---- give it the key, with the password this one time --------------------
Type "l"
Sleep 1.5s
Type "Y"
Sleep 1.2s
Down
Down
Down
Down
Sleep 1s
Enter
Sleep 2.5s
Type "raspberry"
Enter
Sleep 3.5s

# ---- and in, on the key alone --------------------------------------------
Type "h"
Sleep 500ms
Type "/"
Type "raspi"
Sleep 600ms
Enter
Sleep 500ms
Enter
Sleep 2.5s
Type "ls"
Enter
Sleep 2.5s
Type "exit"
Enter
Sleep 2.5s

Hide
Type "q"
Sleep 1s
