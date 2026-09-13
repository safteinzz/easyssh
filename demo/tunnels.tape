# Forward a port: a forward made from raspi's row, running on the Tunnels tab,
# then turned off and back on with Enter, the line kept either way.
#
#   cd demo && vhs tunnels.tape && ./stage.sh down
#
# `raspi` is the stage's throwaway sshd, so the forward really runs.

Output "../readme-assets/tunnels.gif"

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

# ---- the -L spec, in the order ssh writes it -----------------------------
Type "t"
Sleep 1.5s
Type "15432"
Enter
Sleep 400ms
Enter
Sleep 400ms
Type "5432"
Enter
Sleep 400ms
Type "pgdata"
Sleep 1s
Enter
Sleep 4s

# ---- Enter is the off switch, and the on switch --------------------------
Enter
Sleep 2.5s
Enter
Sleep 3s

# Off again before quitting: a forward outlives essh by design, and this one
# would otherwise stay up against the demo host after the render.
Hide
Enter
Sleep 1.5s
Type "q"
Sleep 1s
