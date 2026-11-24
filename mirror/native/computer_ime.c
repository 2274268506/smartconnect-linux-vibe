/* Original Smart Connect SDL3 adapter. No scrcpy internals or global input hooks. */
#define _GNU_SOURCE
#include <SDL3/SDL.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>

__attribute__((constructor)) static void private_channel(void) {
    const char *value=getenv("SMARTCONNECT_IME_FD");
    if (!value) return;
    char *end; long fd=strtol(value,&end,10);
    if (*end || fd<3 || fd>1048576) return;
    int flags=fcntl((int)fd,F_GETFD);
    if (flags>=0) fcntl((int)fd,F_SETFD,flags|FD_CLOEXEC);
}

static bool committed(SDL_Event *event) {
    if (!event || event->type != SDL_EVENT_TEXT_INPUT) return false;
    const char *fd_text = getenv("SMARTCONNECT_IME_FD");
    if (!fd_text) return false;
    char *end;
    long fd = strtol(fd_text, &end, 10);
    if (*end || fd < 3 || fd > 1048576) return false;
    /* scrcpy shortcuts must retain their own key handling. */
    SDL_Keymod (*mods)(void) = dlsym(RTLD_NEXT, "SDL_GetModState");
    if (mods && (mods() & (SDL_KMOD_CTRL | SDL_KMOD_ALT | SDL_KMOD_GUI))) return false;
    const char *text = event->text.text;
    size_t size = text ? strnlen(text, 8193) : 0;
    if (size && size <= 8192) {
        ssize_t result;
        do { result = send((int) fd, text, size, MSG_DONTWAIT | MSG_NOSIGNAL); } while (result < 0 && errno == EINTR);
        if (result != (ssize_t) size) fputs("Smart Connect: computer input bridge unavailable or busy\n", stderr);
    }
    /* Consume commits exactly once: SDK inject-text cannot deliver arbitrary Unicode. */
    return true;
}

bool SDL_WaitEvent(SDL_Event *event) {
    bool (*next)(SDL_Event *) = dlsym(RTLD_NEXT, "SDL_WaitEvent");
    if (!next) return false;
    while (next(event)) { if (!committed(event)) return true; }
    return false;
}

bool SDL_PollEvent(SDL_Event *event) {
    bool (*next)(SDL_Event *) = dlsym(RTLD_NEXT, "SDL_PollEvent");
    if (!next) return false;
    while (next(event)) { if (!committed(event)) return true; }
    return false;
}
