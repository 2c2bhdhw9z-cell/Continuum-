// Continuum — see vulkan_stub_renderer.h.

#ifdef CONTINUUM_HAVE_VULKAN

#include "vulkan_stub_renderer.h"

#include <cmath>
#include <cstdio>

namespace continuum {
namespace {

constexpr double kFramesPerRevolution = 120.0;

// Same hue walk as the host renderer, in float for `VkClearColorValue`.
void HueToRgbF(double hue, float* out) {
  const double h = std::fmod(std::fmax(hue, 0.0), 1.0) * 6.0;
  const int sector = static_cast<int>(h);
  const float f = static_cast<float>(h - sector);
  switch (sector % 6) {
    case 0: out[0] = 1.0f;    out[1] = f;       out[2] = 0.0f;    break;
    case 1: out[0] = 1.0f - f; out[1] = 1.0f;   out[2] = 0.0f;    break;
    case 2: out[0] = 0.0f;    out[1] = 1.0f;    out[2] = f;       break;
    case 3: out[0] = 0.0f;    out[1] = 1.0f - f; out[2] = 1.0f;   break;
    case 4: out[0] = f;       out[1] = 0.0f;    out[2] = 1.0f;    break;
    default: out[0] = 1.0f;   out[1] = 0.0f;    out[2] = 1.0f - f; break;
  }
  out[3] = 1.0f;
}

// The format the compositor expects, and the one CAMetalLayer wants: matching them avoids
// a conversion pass. See §8 of the design document.
constexpr VkFormat kTargetFormat = VK_FORMAT_B8G8R8A8_UNORM;

}  // namespace

#define LOAD(name)                                                                 \
  do {                                                                             \
    name##_ = reinterpret_cast<PFN_##name>(                                        \
        context_.get_device_proc(context_.device, #name));                          \
    if (name##_ == nullptr) {                                                      \
      std::fprintf(stderr, "[stub-vk] missing device function %s\n", #name);       \
      return false;                                                                \
    }                                                                              \
  } while (0)

bool VulkanStubRenderer::LoadFunctions() {
  LOAD(vkCreateImage);
  LOAD(vkDestroyImage);
  LOAD(vkCreateImageView);
  LOAD(vkDestroyImageView);
  LOAD(vkAllocateMemory);
  LOAD(vkFreeMemory);
  LOAD(vkBindImageMemory);
  LOAD(vkGetImageMemoryRequirements);
  LOAD(vkCreateCommandPool);
  LOAD(vkDestroyCommandPool);
  LOAD(vkAllocateCommandBuffers);
  LOAD(vkBeginCommandBuffer);
  LOAD(vkEndCommandBuffer);
  LOAD(vkResetCommandBuffer);
  LOAD(vkCmdPipelineBarrier);
  LOAD(vkCmdClearColorImage);
  LOAD(vkQueueSubmit);
  LOAD(vkCreateSemaphore);
  LOAD(vkDestroySemaphore);
  LOAD(vkCreateFence);
  LOAD(vkDestroyFence);
  LOAD(vkWaitForFences);
  LOAD(vkResetFences);
  LOAD(vkDeviceWaitIdle);

  // Instance-level, so it comes from the other resolver.
  vkGetPhysicalDeviceMemoryProperties_ =
      reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(
          context_.get_instance_proc(context_.instance,
                                     "vkGetPhysicalDeviceMemoryProperties"));
  if (vkGetPhysicalDeviceMemoryProperties_ == nullptr) {
    std::fprintf(stderr, "[stub-vk] missing vkGetPhysicalDeviceMemoryProperties\n");
    return false;
  }
  return true;
}

#undef LOAD

bool VulkanStubRenderer::Initialise(const ExternalVulkanContext& context) {
  context_ = context;
  device_ = static_cast<VkDevice>(context.device);
  gpu_ = static_cast<VkPhysicalDevice>(context.physical_device);
  queue_ = static_cast<VkQueue>(context.queue);

  if (device_ == VK_NULL_HANDLE || queue_ == VK_NULL_HANDLE ||
      context_.get_device_proc == nullptr || context_.get_instance_proc == nullptr) {
    std::fprintf(stderr, "[stub-vk] incomplete Vulkan context from the frontend\n");
    return false;
  }
  if (!LoadFunctions()) return false;

  VkCommandPoolCreateInfo pool_info{};
  pool_info.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
  pool_info.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
  pool_info.queueFamilyIndex = context_.queue_family_index;
  if (vkCreateCommandPool_(device_, &pool_info, nullptr, &command_pool_) != VK_SUCCESS) {
    std::fprintf(stderr, "[stub-vk] vkCreateCommandPool failed\n");
    return false;
  }
  return true;
}

std::uint32_t VulkanStubRenderer::FindMemoryType(std::uint32_t type_bits,
                                                 VkMemoryPropertyFlags properties) {
  VkPhysicalDeviceMemoryProperties memory{};
  vkGetPhysicalDeviceMemoryProperties_(gpu_, &memory);
  for (std::uint32_t i = 0; i < memory.memoryTypeCount; ++i) {
    if ((type_bits & (1u << i)) != 0 &&
        (memory.memoryTypes[i].propertyFlags & properties) == properties) {
      return i;
    }
  }
  return UINT32_MAX;
}

bool VulkanStubRenderer::CreateTarget(Target* target) {
  VkImageCreateInfo image_info{};
  image_info.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
  image_info.imageType = VK_IMAGE_TYPE_2D;
  image_info.format = kTargetFormat;
  image_info.extent = {width_, height_, 1};
  image_info.mipLevels = 1;
  image_info.arrayLayers = 1;
  image_info.samples = VK_SAMPLE_COUNT_1_BIT;
  image_info.tiling = VK_IMAGE_TILING_OPTIMAL;
  // TRANSFER_DST for the clear, SAMPLED because the frontend's composite pass reads it.
  image_info.usage = VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT |
                     VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT;
  image_info.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
  image_info.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;

  if (vkCreateImage_(device_, &image_info, nullptr, &target->image) != VK_SUCCESS) {
    return false;
  }

  VkMemoryRequirements requirements{};
  vkGetImageMemoryRequirements_(device_, target->image, &requirements);
  const std::uint32_t type =
      FindMemoryType(requirements.memoryTypeBits, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
  if (type == UINT32_MAX) return false;

  VkMemoryAllocateInfo allocate{};
  allocate.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
  allocate.allocationSize = requirements.size;
  allocate.memoryTypeIndex = type;
  if (vkAllocateMemory_(device_, &allocate, nullptr, &target->memory) != VK_SUCCESS) {
    return false;
  }
  if (vkBindImageMemory_(device_, target->image, target->memory, 0) != VK_SUCCESS) {
    return false;
  }

  VkImageViewCreateInfo view_info{};
  view_info.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
  view_info.image = target->image;
  view_info.viewType = VK_IMAGE_VIEW_TYPE_2D;
  view_info.format = kTargetFormat;
  view_info.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
  if (vkCreateImageView_(device_, &view_info, nullptr, &target->view) != VK_SUCCESS) {
    return false;
  }

  VkCommandBufferAllocateInfo cmd_info{};
  cmd_info.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
  cmd_info.commandPool = command_pool_;
  cmd_info.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
  cmd_info.commandBufferCount = 1;
  if (vkAllocateCommandBuffers_(device_, &cmd_info, &target->command_buffer) != VK_SUCCESS) {
    return false;
  }

  VkSemaphoreCreateInfo semaphore_info{};
  semaphore_info.sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO;
  if (vkCreateSemaphore_(device_, &semaphore_info, nullptr, &target->done) != VK_SUCCESS) {
    return false;
  }

  VkFenceCreateInfo fence_info{};
  fence_info.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
  if (vkCreateFence_(device_, &fence_info, nullptr, &target->in_flight) != VK_SUCCESS) {
    return false;
  }
  return true;
}

void VulkanStubRenderer::DestroyTarget(Target* target) {
  if (target->in_flight != VK_NULL_HANDLE) vkDestroyFence_(device_, target->in_flight, nullptr);
  if (target->done != VK_NULL_HANDLE) vkDestroySemaphore_(device_, target->done, nullptr);
  if (target->view != VK_NULL_HANDLE) vkDestroyImageView_(device_, target->view, nullptr);
  if (target->image != VK_NULL_HANDLE) vkDestroyImage_(device_, target->image, nullptr);
  if (target->memory != VK_NULL_HANDLE) vkFreeMemory_(device_, target->memory, nullptr);
  *target = Target{};
}

bool VulkanStubRenderer::Configure(std::uint32_t width, std::uint32_t height) {
  width_ = width;
  height_ = height;
  for (std::size_t i = 0; i < kTargetCount; ++i) {
    if (!CreateTarget(&targets_[i])) {
      std::fprintf(stderr, "[stub-vk] could not create target %zu\n", i);
      return false;
    }
  }
  return true;
}

void VulkanStubRenderer::Render(std::uint64_t frame_index, PresentedFrame* out) {
  Target& target = targets_[next_target_];
  next_target_ = (next_target_ + 1) % kTargetCount;

  // Wait for this target's *previous* use, not for the frame just submitted. That is what
  // double buffering buys: the CPU is a frame ahead of the GPU rather than lockstep.
  if (target.ever_submitted) {
    vkWaitForFences_(device_, 1, &target.in_flight, VK_TRUE, UINT64_MAX);
    vkResetFences_(device_, 1, &target.in_flight);
  }

  vkResetCommandBuffer_(target.command_buffer, 0);

  VkCommandBufferBeginInfo begin{};
  begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
  begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
  vkBeginCommandBuffer_(target.command_buffer, &begin);

  // UNDEFINED (or SHADER_READ_ONLY from the last pass) → TRANSFER_DST for the clear.
  // Discarding via UNDEFINED is correct and cheaper than preserving contents we overwrite.
  VkImageMemoryBarrier to_transfer{};
  to_transfer.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
  to_transfer.oldLayout = VK_IMAGE_LAYOUT_UNDEFINED;
  to_transfer.newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL;
  to_transfer.srcAccessMask = 0;
  to_transfer.dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
  to_transfer.image = target.image;
  to_transfer.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
  to_transfer.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
  to_transfer.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
  vkCmdPipelineBarrier_(target.command_buffer, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                        VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1,
                        &to_transfer);

  VkClearColorValue colour{};
  HueToRgbF(static_cast<double>(frame_index) / kFramesPerRevolution, colour.float32);
  VkImageSubresourceRange range{VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
  vkCmdClearColorImage_(target.command_buffer, target.image,
                        VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &colour, 1, &range);

  // → SHADER_READ_ONLY, which is the layout the frontend is told to expect. Declaring
  // anything else would cost it an extra barrier of its own.
  VkImageMemoryBarrier to_sample = to_transfer;
  to_sample.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL;
  to_sample.newLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
  to_sample.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
  to_sample.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
  vkCmdPipelineBarrier_(target.command_buffer, VK_PIPELINE_STAGE_TRANSFER_BIT,
                        VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT, 0, 0, nullptr, 0, nullptr, 1,
                        &to_sample);

  vkEndCommandBuffer_(target.command_buffer);

  VkSubmitInfo submit{};
  submit.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
  submit.commandBufferCount = 1;
  submit.pCommandBuffers = &target.command_buffer;
  // The frontend waits on this on the GPU rather than the CPU.
  submit.signalSemaphoreCount = 1;
  submit.pSignalSemaphores = &target.done;

  // The single most important two lines in this file. A VkQueue is not thread-safe, and
  // under MoltenVK a VkQueue *is* an MTLCommandQueue that the frontend's compositor also
  // submits to. Every engine submission goes inside this lock.
  if (context_.lock_queue != nullptr) context_.lock_queue(context_.queue_lock_opaque);
  const VkResult submitted = vkQueueSubmit_(queue_, 1, &submit, target.in_flight);
  if (context_.unlock_queue != nullptr) context_.unlock_queue(context_.queue_lock_opaque);

  if (submitted != VK_SUCCESS) {
    std::fprintf(stderr, "[stub-vk] vkQueueSubmit failed: %d\n", submitted);
    return;
  }
  target.ever_submitted = true;

  out->image = target.image;
  out->image_view = target.view;
  out->wait_semaphore = target.done;
  out->image_layout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
  out->width = width_;
  out->height = height_;
  out->software_pixels = nullptr;
}

void VulkanStubRenderer::Shutdown() {
  if (device_ == VK_NULL_HANDLE) return;
  // Idle before destroying anything the GPU may still be reading.
  vkDeviceWaitIdle_(device_);
  for (std::size_t i = 0; i < kTargetCount; ++i) DestroyTarget(&targets_[i]);
  if (command_pool_ != VK_NULL_HANDLE) {
    vkDestroyCommandPool_(device_, command_pool_, nullptr);
    command_pool_ = VK_NULL_HANDLE;
  }
  device_ = VK_NULL_HANDLE;
}

}  // namespace continuum

#endif  // CONTINUUM_HAVE_VULKAN
