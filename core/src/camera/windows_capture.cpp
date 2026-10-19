#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <objbase.h>

#include "windows_capture.h"

#include <algorithm>
#include <atomic>
#include <cstdio>
#include <exception>
#include <memory>

#include "api/video/video_frame.h"
#include "api/video/video_frame_buffer.h"
#include "api/video/video_sink_interface.h"
#include "modules/video_capture/video_capture_factory.h"

namespace {

void write_error(char* error, size_t error_len, const char* message) {
  if (error != nullptr && error_len > 0) {
    std::snprintf(error, error_len, "%s", message);
  }
}

// COM initialization belongs to each calling thread, not to the process.
class ComApartment {
 public:
  ComApartment()
      : result_(CoInitializeEx(nullptr, COINIT_MULTITHREADED)) {}
  ~ComApartment() {
    if (SUCCEEDED(result_)) {
      CoUninitialize();
    }
  }

  bool initialize_error(char* error, size_t error_len) const {
    if (SUCCEEDED(result_)) {
      return false;
    }
    if (error != nullptr && error_len > 0) {
      std::snprintf(error, error_len,
                    "Camera COM initialization failed (HRESULT 0x%08lX)",
                    static_cast<unsigned long>(result_));
    }
    return true;
  }

 private:
  HRESULT result_;
};

}  // namespace

struct HoppWindowsCamera : webrtc::VideoSinkInterface<webrtc::VideoFrame> {
  // Declared first so COM outlives every WebRTC object during destruction.
  ComApartment apartment;
  webrtc::scoped_refptr<webrtc::VideoCaptureModule> module;
  HoppWindowsCameraFrameCallback callback;
  void* userdata;
  std::atomic<bool> stopping{false};

  HoppWindowsCamera(HoppWindowsCameraFrameCallback frame_callback,
                    void* frame_userdata)
      : callback(frame_callback), userdata(frame_userdata) {}

  ~HoppWindowsCamera() override {
    stopping.store(true, std::memory_order_release);
    if (module) {
      // Deregistration synchronizes with delivery in VideoCaptureImpl. Keep the
      // sink and Rust callback context alive until the graph has stopped.
      module->DeRegisterCaptureDataCallback();
      module->StopCapture();
      module = nullptr;
    }
  }

  void OnFrame(const webrtc::VideoFrame& frame) override {
    if (stopping.load(std::memory_order_acquire)) {
      return;
    }
    const auto buffer = frame.video_frame_buffer()->ToI420();
    if (!buffer) {
      return;
    }
    callback(userdata, buffer->width(), buffer->height(), buffer->DataY(),
             buffer->StrideY(), buffer->DataU(), buffer->StrideU(),
             buffer->DataV(), buffer->StrideV(), frame.timestamp_us());
  }
};

extern "C" int hopp_windows_camera_list(
    HoppWindowsCameraDeviceCallback callback,
    void* userdata,
    char* error,
    size_t error_len) {
  write_error(error, error_len, "");
  if (callback == nullptr) {
    write_error(error, error_len, "Missing camera enumeration callback");
    return -1;
  }
  try {
    ComApartment apartment;
    if (apartment.initialize_error(error, error_len)) {
      return -1;
    }
    std::unique_ptr<webrtc::VideoCaptureModule::DeviceInfo> devices(
        webrtc::VideoCaptureFactory::CreateDeviceInfo());
    if (!devices) {
      write_error(error, error_len, "Failed to enumerate DirectShow cameras");
      return -1;
    }
    const auto count = devices->NumberOfDevices();
    for (uint32_t index = 0; index < count; ++index) {
      char name[webrtc::kVideoCaptureDeviceNameLength] = {};
      char id[webrtc::kVideoCaptureUniqueNameLength] = {};
      if (devices->GetDeviceName(index, name, sizeof(name), id, sizeof(id)) !=
          0) {
        write_error(error, error_len, "Failed to read DirectShow camera name");
        return -1;
      }
      callback(name, id, userdata);
    }
    return static_cast<int>(count);
  } catch (const std::exception& exception) {
    write_error(error, error_len, exception.what());
  } catch (...) {
    write_error(error, error_len, "Unexpected camera enumeration failure");
  }
  return -1;
}

extern "C" HoppWindowsCamera* hopp_windows_camera_start(
    const char* device_id,
    int width,
    int height,
    int fps,
    HoppWindowsCameraFrameCallback callback,
    void* userdata,
    char* error,
    size_t error_len) {
  write_error(error, error_len, "");
  if (device_id == nullptr || device_id[0] == '\0' || callback == nullptr ||
      width <= 0 || height <= 0 || fps <= 0) {
    write_error(error, error_len, "Invalid camera capture parameters");
    return nullptr;
  }
  try {
    auto camera = std::make_unique<HoppWindowsCamera>(callback, userdata);
    if (camera->apartment.initialize_error(error, error_len)) {
      return nullptr;
    }
    std::unique_ptr<webrtc::VideoCaptureModule::DeviceInfo> devices(
        webrtc::VideoCaptureFactory::CreateDeviceInfo());
    if (!devices) {
      write_error(error, error_len, "Failed to query camera capabilities");
      return nullptr;
    }
    webrtc::VideoCaptureCapability requested;
    requested.width = width;
    requested.height = height;
    requested.maxFPS = fps;
    webrtc::VideoCaptureCapability selected;
    if (devices->GetBestMatchedCapability(device_id, requested, selected) < 0) {
      write_error(error, error_len, "Camera has no supported capture format");
      return nullptr;
    }
    // Capabilities report the mode's maximum frame rate, not our requested
    // rate. Preserve the request when configuring the native capture graph.
    selected.maxFPS = std::min(requested.maxFPS, selected.maxFPS);
    camera->module = webrtc::VideoCaptureFactory::Create(device_id);
    if (!camera->module) {
      write_error(error, error_len, "Failed to open DirectShow camera");
      return nullptr;
    }
    camera->module->RegisterCaptureDataCallback(camera.get());
    if (camera->module->StartCapture(selected) != 0) {
      write_error(error, error_len, "Failed to start DirectShow camera capture");
      return nullptr;
    }
    return camera.release();
  } catch (const std::exception& exception) {
    write_error(error, error_len, exception.what());
  } catch (...) {
    write_error(error, error_len, "Unexpected camera capture failure");
  }
  return nullptr;
}

extern "C" void hopp_windows_camera_stop(HoppWindowsCamera* camera) {
  delete camera;
}
