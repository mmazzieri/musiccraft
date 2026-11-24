// MusicCraft's library (crates/ffi): one call, JSON in and JSON out. See docs/mobile.md.
#pragma once

// Runs one request. The answer is never null and goes back to musiccraft_free once.
char *musiccraft_call(const char *request);

void musiccraft_free(char *answer);
