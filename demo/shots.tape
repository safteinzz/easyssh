# Every README screenshot, rendered by VHS instead of taken by hand. No tmux, no
# window sizing, no cropping: the size below is the size of every shot, and a
# rerun after a UI change replaces them all identically.
#
#   cd demo && vhs shots.tape
#
# The files are written straight into readme-assets/. The tunnel and the mount
# shots need `raspi` to be a real machine, so export ESSH_DEMO_HOST first - see
# the header of stage.sh. The frames only ever show the alias, never an address.

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
Type "./stage.sh shell"
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

# ---- Hosts ---------------------------------------------------------------
Screenshot ../readme-assets/hosts.png
Sleep 700ms

# ---- Keys ----------------------------------------------------------------
Hide
Type "l"
Sleep 1s
Show
Sleep 500ms
Screenshot ../readme-assets/keys.png
Sleep 700ms

# ---- the mount wizard ----------------------------------------------------
Hide
Type "h"
Sleep 500ms
Type "/"
Type "raspi"
Enter
Sleep 800ms
Type "m"
Sleep 1s
Show
Sleep 500ms
Screenshot ../readme-assets/wizard-mount.png
Sleep 700ms

# ---- Mounts --------------------------------------------------------------
Hide
Enter
Sleep 300ms
Enter
Sleep 300ms
Enter
Sleep 4s
Show
Sleep 500ms
Screenshot ../readme-assets/mounts.png
Sleep 700ms

# ---- Tunnels -------------------------------------------------------------
Hide
Type "d"
Sleep 2s
Type "h"
Sleep 300ms
Type "h"
Sleep 300ms
Type "h"
Sleep 500ms
Type "/"
Type "raspi"
Enter
Sleep 500ms
Type "t"
Sleep 500ms
Type "15432"
Enter
Sleep 300ms
Enter
Sleep 3s
Show
Sleep 500ms
Screenshot ../readme-assets/tunnels.png
Sleep 700ms

# ---- Settings ------------------------------------------------------------
Hide
Type "d"
Sleep 1.5s
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
