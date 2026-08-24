# A scripted tour of the toolbox, rendered by VHS. Nothing here is recorded off
# a screen: VHS runs these keystrokes in a headless terminal and draws the GIF
# itself, so it re-renders identically every release.
#
#   cd demo && vhs demo.tape
#
# One tab, one thing it does, in the order you would meet them. Everything runs
# inside the staged home; the tape never tears it down.
#
# The tunnel, the mount and the connect all need `raspi` to be a real machine,
# so export ESSH_DEMO_HOST first - see the header of stage.sh. Every other host
# stays fake. Note the connect at the end shows that machine's login banner and
# prompt to everyone who reads the README: drop the last scene if you would
# rather it did not.

Output "../readme-assets/demo.gif"

Set Shell bash
# The prompt is drawn with nerd-font glyphs; without this they render as boxes.
Set FontFamily "JetBrainsMono NF"
Set FontSize 15
Set Width 1500
Set Height 760
Set Padding 25
Set Framerate 24
Set TypingSpeed 60ms

# ---- setup, off camera ----------------------------------------------------
Hide
Type "./stage.sh up >/dev/null 2>&1"
Enter
Sleep 4s
Type "./stage.sh shell"
Enter
Sleep 3s
Type "clear"
Enter
Sleep 500ms
Show

# ===========================================================================
# SCENE 1 - Hosts: your config is already here
# ===========================================================================
Type "essh"
Sleep 300ms
Enter
Sleep 1s

# The panel follows the cursor: up or down, last connect, key, jump host.
Down
Sleep 1.2s
Down
Sleep 1.2s
Down
Sleep 1.2s

# and then you get bored, which is what `/` is for
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 900ms


# ===========================================================================
# SCENE 2 - find it, and mount it
# ===========================================================================
Type "/"
Sleep 400ms
Type "raspi"
Sleep 1.5s
Enter
Sleep 1.2s

Type "m"
Sleep 2s
Enter
Sleep 400ms
Enter
Sleep 400ms
Enter
Sleep 3.5s

# ===========================================================================
# SCENE 3 - and gone again
# ===========================================================================
Type "d"
Sleep 2.5s

# ===========================================================================
# SCENE 4 - a port forward, made and killed
# ===========================================================================
# The way back to Hosts passes Keys, so stop and let it be read: what is in
# ~/.ssh, what the agent holds, what will ask for a passphrase.
Type "h"
Sleep 400ms
Type "h"
Sleep 2.5s
Down
Sleep 1.5s
Type "h"
Sleep 700ms
Down
Sleep 250ms
Down
Sleep 800ms

Type "t"
Sleep 1.5s
Type "5432"
Sleep 1.2s
Enter
Sleep 400ms
Enter
Sleep 3s

# 5432 is taken on this machine, so ssh refuses the forward and essh says so
# instead of listing a tunnel that carries nothing. The wizard stays open on the
# port that failed, still on the local-port field - which is the one that was
# busy - so the fix is typing a free one.
Type "15432"
Sleep 1.2s
Enter
Sleep 400ms
Enter
Sleep 2.5s

# a real background ssh -N, already selected, and `d` is the only easy way to
# end one
Type "d"
Sleep 2s

# ===========================================================================
# SCENE 5 - the defaults are yours
# ===========================================================================
Type "l"
Sleep 300ms
Type "l"
Sleep 1.5s
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 1s
Enter
Sleep 1s
Backspace 5
Sleep 700ms
Type "mnt"
Sleep 1.2s
Enter
Sleep 1.2s

# ===========================================================================
# SCENE 6 - the new default, and the point of all of it
# ===========================================================================
Type "l"
Sleep 600ms
Type "/"
Sleep 250ms
Type "raspi"
Sleep 900ms
Enter
Sleep 500ms

# the mount wizard now offers ~/mnt/raspi, because Settings says so
Type "m"
Sleep 1s
Enter
Sleep 2.5s
Escape
Sleep 800ms

Enter
Sleep 4s
Type "exit"
Enter
Sleep 2s
Type "q"
Sleep 1s
