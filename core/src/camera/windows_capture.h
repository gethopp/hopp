#ifndef HOPP_WINDOWS_CAPTURE_H
#define HOPP_WINDOWS_CAPTURE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct HoppWindowsCamera HoppWindowsCamera;

// Strings and frame planes are borrowed for the duration of their callback.
typedef void (*HoppWindowsCameraDeviceCallback)(const char* name,
                                               const char* id,
                                               void* userdata);
typedef void (*HoppWindowsCameraFrameCallback)(void* userdata,
                                              int width,
                                              int height,
                                              const uint8_t* y,
                                              int stride_y,
                                              const uint8_t* u,
                                              int stride_u,
                                              const uint8_t* v,
                                              int stride_v,
                                              int64_t timestamp_us);

// Call on a worker thread that can enter the COM multithreaded apartment.
// Returns the device count, or -1 with a null-terminated error message.
int hopp_windows_camera_list(HoppWindowsCameraDeviceCallback callback,
                             void* userdata,
                             char* error,
                             size_t error_len);

// Uses the closest supported device capability. The handle owns COM until stop.
// Returns NULL on failure. Frame callbacks run on WebRTC's capture thread and
// must return promptly, must not throw/panic, and must not call stop themselves.
HoppWindowsCamera* hopp_windows_camera_start(
    const char* device_id,
    int width,
    int height,
    int fps,
    HoppWindowsCameraFrameCallback callback,
    void* userdata,
    char* error,
    size_t error_len);

// Call on the same thread as start. No callback can run after this returns.
// Capture is callback-based, so stop never waits for the next camera frame.
void hopp_windows_camera_stop(HoppWindowsCamera* camera);

#ifdef __cplusplus
}
#endif

#endif
