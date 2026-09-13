# The README's two stills, Keys and Settings, rendered by VHS instead of taken
# by hand. The rest of the pictures are GIFs, one tape each.
#
#   cd demo && vhs shots.tape
#
# The files are written straight into readme-assets/.

# Roughly 130 columns by 22 rows: wide enough for the detail panel, short enough
# to stay readable when GitLab scales it down.
# VHS always records a video; with every frame hidden there is nothing to
# encode and it stops with "no frames". This one is a throwaway.
Output "/tmp/vhs-shots-throwaway.gif"

Set Shell bash
Set FontFamily "JetBrainsMono NF"
Set Theme "Catppuccin Mocha"
Set FontSize 15
Set Width 1240
Set Height 520
Set Padding 20

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
Type "clear"
Enter
Sleep 500ms
Type "essh"
Enter
Sleep 3s
Show
Sleep 500ms

# ---- Keys ----------------------------------------------------------------
Hide
Type "l"
Sleep 1s
Show
Sleep 500ms
Screenshot ../readme-assets/keys.png
Sleep 700ms

# ---- Settings ------------------------------------------------------------
Hide
Type "l"
Sleep 300ms
Type "l"
Sleep 300ms
Type "l"
Sleep 800ms
Down
Down
Sleep 800ms
Show
Sleep 500ms
Screenshot ../readme-assets/settings.png
Sleep 700ms
