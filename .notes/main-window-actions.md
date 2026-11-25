# Main-window exit action

## What and why

Expose Quit in the main header so users can exit without a system-tray host. The user subsequently requested removal of application restart buttons. Remove the header restart action, the language-preference restart button, its process-launch helper and unused translations.

## How

`gui/src/window.rs` routes Quit through existing tray cleanup and managed-daemon stop. Closing the window still hides it to the tray. Language changes persist immediately and apply after the user quits and opens the app again; preferences explain this. Keep the responsive page-switcher layout.

## Validation

Run workspace release build/tests and isolated window close/reopen and language-preference checks. The latter confirms saved language survives reopening preferences without a restart button. Do not stop the live application/service during validation.
