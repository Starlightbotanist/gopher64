#include "wsi_platform.hpp"
#include <SDL3/SDL_vulkan.h>

VkSurfaceKHR SDL_WSIPlatform::create_surface(VkInstance instance,
                                             VkPhysicalDevice gpu) {
  VkSurfaceKHR surface = nullptr;
#ifdef __ANDROID__
  if (vk_get_instance_proc_addr) {
    auto create_surface = reinterpret_cast<PFN_vkCreateAndroidSurfaceKHR>(
        vk_get_instance_proc_addr(instance, "vkCreateAndroidSurfaceKHR"));
    auto *native_window = static_cast<ANativeWindow *>(SDL_GetPointerProperty(
        SDL_GetWindowProperties(window), SDL_PROP_WINDOW_ANDROID_WINDOW_POINTER,
        nullptr));
    if (!create_surface || !native_window) {
      printf("Error resolving Android Vulkan surface functions\n");
      return nullptr;
    }
    VkAndroidSurfaceCreateInfoKHR create_info = {};
    create_info.sType = VK_STRUCTURE_TYPE_ANDROID_SURFACE_CREATE_INFO_KHR;
    create_info.window = native_window;
    if (create_surface(instance, &create_info, nullptr, &surface) != VK_SUCCESS) {
      printf("Error creating Android Vulkan surface\n");
      return nullptr;
    }
    return surface;
  }
#endif
  bool result = SDL_Vulkan_CreateSurface(window, instance, NULL, &surface);
  if (result != true) {
    printf("Error creating surface\n");
  }
  return surface;
}

void SDL_WSIPlatform::destroy_surface(VkInstance instance,
                                      VkSurfaceKHR surface) {
#ifdef __ANDROID__
  if (vk_get_instance_proc_addr) {
    auto destroy_surface = reinterpret_cast<PFN_vkDestroySurfaceKHR>(
        vk_get_instance_proc_addr(instance, "vkDestroySurfaceKHR"));
    if (destroy_surface) {
      destroy_surface(instance, surface, nullptr);
    }
    return;
  }
#endif
  SDL_Vulkan_DestroySurface(instance, surface, NULL);
}

std::vector<const char *> SDL_WSIPlatform::get_instance_extensions() {
#ifdef __ANDROID__
  if (vk_get_instance_proc_addr) {
    return {VK_KHR_SURFACE_EXTENSION_NAME, VK_KHR_ANDROID_SURFACE_EXTENSION_NAME};
  }
#endif

  unsigned int extensionCount = 0;
  char const *const *extensions =
      SDL_Vulkan_GetInstanceExtensions(&extensionCount);
  if (extensions == NULL) {
    printf("Error getting instance extensions\n");
  }

  std::vector<const char *> extensionNames;
  for (unsigned int i = 0; i < extensionCount; ++i) {
    extensionNames.push_back(extensions[i]);
  }
  return extensionNames;
}

uint32_t SDL_WSIPlatform::get_surface_width() {
  int w, h;
  SDL_GetWindowSize(window, &w, &h);
  return w;
}

uint32_t SDL_WSIPlatform::get_surface_height() {
  int w, h;
  SDL_GetWindowSize(window, &w, &h);
  return h;
}

bool SDL_WSIPlatform::alive(Vulkan::WSI &wsi) { return true; }

void SDL_WSIPlatform::poll_input() { SDL_PumpEvents(); }

void SDL_WSIPlatform::poll_input_async(Granite::InputTrackerHandler *handler) {}

void SDL_WSIPlatform::set_window(SDL_Window *_window) { window = _window; }

void SDL_WSIPlatform::set_vk_get_instance_proc_addr(
    PFN_vkGetInstanceProcAddr proc_addr) {
  vk_get_instance_proc_addr = proc_addr;
}

void SDL_WSIPlatform::do_resize() { resize = true; }
