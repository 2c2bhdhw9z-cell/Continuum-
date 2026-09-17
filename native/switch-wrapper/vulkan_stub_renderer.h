// Continuum — the real Vulkan handover, exercised by a clear.
//
// Compiled only when CONTINUUM_HAVE_VULKAN is defined, which on the target build means
// MoltenVK's headers are present. Everything here uses the injected device: this type
// never creates an instance, a device or a queue, because on iOS there is exactly one GPU
// and every layer in the process must share the one MTLDevice the Swift layer made.
//
// The work it does is a `vkCmdClearColorImage` into a frontend-owned image, then a submit
// that signals a semaphore the frontend waits on. That is deliberately the smallest thing
// that is still a genuine hardware frame: image acquisition, a layout transition, a queue
// submission under `lock_queue`, and a GPU-side completion signal.
//
// See docs/SET_HW_RENDER_DESIGN.md §12.5.

#ifndef CONTINUUM_VULKAN_STUB_RENDERER_H
#define CONTINUUM_VULKAN_STUB_RENDERER_H

#ifdef CONTINUUM_HAVE_VULKAN

#include <vulkan/vulkan_core.h>

#include <cstdint>
#include <vector>

#include "switch_engine.h"

namespace continuum {

class VulkanStubRenderer {
 public:
  bool Initialise(const ExternalVulkanContext& context);
  void Shutdown();

  // Allocates the double-buffered targets. Called once; the frontend's allocation cannot
  // grow mid-session, so this uses the declared maximum.
  bool Configure(std::uint32_t width, std::uint32_t height);

  void Render(std::uint64_t frame_index, PresentedFrame* out);

 private:
  struct Target {
    VkImage image = VK_NULL_HANDLE;
    VkImageView view = VK_NULL_HANDLE;
    VkDeviceMemory memory = VK_NULL_HANDLE;
    VkCommandBuffer command_buffer = VK_NULL_HANDLE;
    VkSemaphore done = VK_NULL_HANDLE;
    VkFence in_flight = VK_NULL_HANDLE;
    bool ever_submitted = false;
  };

  bool LoadFunctions();
  bool CreateTarget(Target* target);
  void DestroyTarget(Target* target);
  std::uint32_t FindMemoryType(std::uint32_t type_bits, VkMemoryPropertyFlags properties);

  ExternalVulkanContext context_{};
  VkDevice device_ = VK_NULL_HANDLE;
  VkPhysicalDevice gpu_ = VK_NULL_HANDLE;
  VkQueue queue_ = VK_NULL_HANDLE;
  VkCommandPool command_pool_ = VK_NULL_HANDLE;

  std::uint32_t width_ = 0;
  std::uint32_t height_ = 0;

  // Two, alternating. Without double buffering the compositor can be sampling frame n
  // while this writes n+1, which on a tiler shows as tearing inside one presented frame.
  static constexpr std::size_t kTargetCount = 2;
  Target targets_[kTargetCount]{};
  std::size_t next_target_ = 0;

  // Resolved from the injected `get_device_proc`, never from a link-time symbol: the
  // loader in the app is MoltenVK's, and linking directly would bypass the device the
  // frontend created.
  PFN_vkCreateImage vkCreateImage_ = nullptr;
  PFN_vkDestroyImage vkDestroyImage_ = nullptr;
  PFN_vkCreateImageView vkCreateImageView_ = nullptr;
  PFN_vkDestroyImageView vkDestroyImageView_ = nullptr;
  PFN_vkAllocateMemory vkAllocateMemory_ = nullptr;
  PFN_vkFreeMemory vkFreeMemory_ = nullptr;
  PFN_vkBindImageMemory vkBindImageMemory_ = nullptr;
  PFN_vkGetImageMemoryRequirements vkGetImageMemoryRequirements_ = nullptr;
  PFN_vkCreateCommandPool vkCreateCommandPool_ = nullptr;
  PFN_vkDestroyCommandPool vkDestroyCommandPool_ = nullptr;
  PFN_vkAllocateCommandBuffers vkAllocateCommandBuffers_ = nullptr;
  PFN_vkBeginCommandBuffer vkBeginCommandBuffer_ = nullptr;
  PFN_vkEndCommandBuffer vkEndCommandBuffer_ = nullptr;
  PFN_vkResetCommandBuffer vkResetCommandBuffer_ = nullptr;
  PFN_vkCmdPipelineBarrier vkCmdPipelineBarrier_ = nullptr;
  PFN_vkCmdClearColorImage vkCmdClearColorImage_ = nullptr;
  PFN_vkQueueSubmit vkQueueSubmit_ = nullptr;
  PFN_vkCreateSemaphore vkCreateSemaphore_ = nullptr;
  PFN_vkDestroySemaphore vkDestroySemaphore_ = nullptr;
  PFN_vkCreateFence vkCreateFence_ = nullptr;
  PFN_vkDestroyFence vkDestroyFence_ = nullptr;
  PFN_vkWaitForFences vkWaitForFences_ = nullptr;
  PFN_vkResetFences vkResetFences_ = nullptr;
  PFN_vkGetPhysicalDeviceMemoryProperties vkGetPhysicalDeviceMemoryProperties_ = nullptr;
  PFN_vkDeviceWaitIdle vkDeviceWaitIdle_ = nullptr;
};

}  // namespace continuum

#endif  // CONTINUUM_HAVE_VULKAN
#endif  // CONTINUUM_VULKAN_STUB_RENDERER_H
