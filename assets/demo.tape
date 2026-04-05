# Demo recording for settl -- terminal hex settlement game
# Generates a GIF showing the TUI launch, game setup, and board.
# Run from repo root: vhs assets/demo.tape
# Requires: vhs (https://github.com/charmbracelet/vhs), a release build

Output assets/demo.gif

Set Shell "bash"
Set FontSize 13
Set Width 1400
Set Height 1100
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

# Wait for title screen to render
Sleep 3s

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

# Wait for llamafile setup to complete (model already cached)
Sleep 6s

# Board is now visible with settlement placement cursor.
# Browse a few legal positions then place.
Type "n"
Sleep 400ms
Type "n"
Sleep 400ms
Type "n"
Sleep 400ms
Enter
Sleep 1500ms

# Road placement prompt appears -- browse and place.
Type "n"
Sleep 400ms
Type "n"
Sleep 400ms
Enter
Sleep 3s

# Quit
Type "q"
Sleep 1s
