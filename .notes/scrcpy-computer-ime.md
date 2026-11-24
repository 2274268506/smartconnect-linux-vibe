# Computer input method in scrcpy windows

## What and why

Phone physical-keyboard composition and computer-side composition are separate choices. SDK text injection rejects arbitrary Chinese characters. Computer mode therefore uses the existing Smart Connect `TEXT_COMMITTED` interface instead of passing confirmed Unicode to that injector.

## How

App/desktop preferences independently store `computerIme` (off by default). It temporarily overrides the saved phone-IME choice. Supported hosts advertise the capability; unsupported hosts/versions fail clearly. Navigation keys and mouse handling remain separate.

An original, optional SDL3 adapter intercepts WaitEvent/PollEvent text commits inside the owned scrcpy process only. `--prefer-text` prevents raw-letter duplication. An unnamed session-owned datagram channel carries bounded UTF-8 text; the host drains it into service 16 command 7 using the current source display ID. Invalid/NUL/oversize data is discarded, shortcut-modified text retains scrcpy handling, and no text reaches logs or the clipboard. Cleanup removes the private adapter file/channel. Phone mode still isolates desktop Fcitx, while computer mode inherits the computer input environment.

The adapter is compiled from original C source with SDL3 headers and embedded into the mirror executable. It requires scrcpy 5+ (SDL3); ordinary native builds without headers omit this optional capability. No forked scrcpy binary is installed. See [SDL text events](https://wiki.libsdl.org/SDL3/SDL_TextInputEvent) and [WaitEvent](https://wiki.libsdl.org/SDL3/SDL_WaitEvent).

## Validation

Release workspace build and 311 automated tests passed, including a synthetic SDL3 Wait/Poll adapter test and Unicode/invalid-packet validation. Two isolated GTK app/desktop persistence tests passed. The installed GUI/host were restarted while idle; existing preferences and daemon connectivity were preserved.

On the connected Android 16 phone with scrcpy 5, the Settings projection used the computer adapter alongside the 150% compatibility mouse. With computer Rime active, typing pinyin and confirming a candidate inserted the expected Chinese text once in the phone search field. A subsequent Backspace removed a committed Chinese character. The own test stream/child and private adapter were removed; computer IME activation was returned to its initial inactive state. Other IMEs and platforms are not claimed as verified. All artifacts remain outside the repository; no real message was sent.
