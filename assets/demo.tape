# Demo recording for settl -- terminal hex settlement game
# Generates a GIF showing the TUI launch, game setup, and board.
# Run from repo root: vhs assets/demo.tape
# Requires: vhs (https://github.com/charmbracelet/vhs), a release build

Output assets/demo.gif

Set Shell "bash"
Set FontSize 20
Set Width 2000
Set Height 1650
Set LineHeight 1.1
Set Padding 10
Set TypingSpeed 50ms
Set Theme "Catppuccin Mocha"

# Run a harmless command first to prevent Enter from leaking into the TUI
Hide
Type "echo"
Enter
Sleep 500ms
Show

# Launch the game
Type "./target/release/settl"
Enter

# Wait for title screen to render, then pause briefly to show it
Wait+Screen /New Game/
Sleep 1500ms

# Select "New Game" (already highlighted)
Enter
Sleep 1500ms

# Navigate down from Start Button to Friendly Robber
# Focus: StartButton -> PlayerCount -> Player1 -> Player2 -> Player3 -> FriendlyRobber
Down
Sleep 200ms
Down
Sleep 200ms
Down
Sleep 200ms
Down
Sleep 200ms
Down
Sleep 400ms

# Now on Friendly Robber -- toggle it on
Right
Sleep 500ms

# Down to Board Layout
Down
Sleep 400ms

# Toggle to Random
Right
Sleep 700ms

# Start the game (Enter works from any row, model stays Bonsai 1.7B)
Enter

# Wait for the board to appear (llamafile downloads/starts in the background)
Wait+Screen /Place settlement/
Sleep 500ms

# Browse a few legal positions then place.
Type "n"
Sleep 400ms
Type "n"
Sleep 400ms
Type "n"
Sleep 400ms
Enter

# Wait for road placement prompt to appear.
Wait+Screen /Place road/
Sleep 500ms

# Browse and place.
Type "n"
Sleep 400ms
Type "n"
Sleep 400ms
Enter
Sleep 3s

# Quit
Type "q"
Sleep 1s
