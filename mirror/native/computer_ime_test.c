/* Synthetic SDL3 commits: no phone, clipboard or real keyboard input. */
#include <SDL3/SDL.h>
int main(void) {
    if (!SDL_Init(SDL_INIT_EVENTS)) return 1;
    SDL_Event event={0}; event.type=SDL_EVENT_TEXT_INPUT; event.text.text="中文🙂";
    if (!SDL_PushEvent(&event)) return 2;
    event.type=SDL_EVENT_USER; if (!SDL_PushEvent(&event)) return 3;
    if (!SDL_WaitEvent(&event) || event.type!=SDL_EVENT_USER) return 4;
    event.type=SDL_EVENT_TEXT_INPUT; event.text.text="ASCII";
    if (!SDL_PushEvent(&event)) return 5;
    /* SDL may expose a poll-cycle sentinel before a newly queued event. */
    for (int i=0;i<8;++i) {if(SDL_PollEvent(&event) && event.type==SDL_EVENT_TEXT_INPUT) return 6;}
    SDL_Quit(); return 0;
}
