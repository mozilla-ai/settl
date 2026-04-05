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

# Wait for title screen
Sleep 5s

# Admire the SETTL logo and menu
Sleep 2s

# Select "New Game" (already highlighted)
Enter
Sleep 2s

# Navigate down from Start Button to Friendly Robber
# Focus: StartButton -> PlayerCount -> Player1 -> Player2 -> Player3 -> FriendlyRobber
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 500ms

# Now on Friendly Robber -- toggle it on
Right
Sleep 700ms

# Down to Board Layout
Down
Sleep 500ms

# Toggle to Random
Right
Sleep 1s

# Start the game (Enter works from any row, model stays Bonsai 1.7B)
Enter

# Wait for llamafile setup to complete (model already cached)
Sleep 10s

# Admire the board
Sleep 4s

# Quit
Type "q"
Sleep 1s
