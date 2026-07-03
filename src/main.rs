#![allow(unsafe_code, unused_variables)]

use std::ffi::CString;

use ash::vk;
use sdl3::Sdl;
use sdl3::video::Window;
use vk_mem::Alloc;

const DISCRETE_NVIDIA_GPU_INDEX_ON_MY_LAPTOP: usize = 1;
const LOG_ALL_AVAILABLE_DEVICES: bool = false;

fn main() {
    let (sdl, window) = init_window().unwrap();

    let renderer = Renderer::init(window).unwrap();

    todo!("enter draw loop");
}

fn init_window() -> Result<(Sdl, Window), Box<dyn std::error::Error>> {
    let sdl = sdl3::init()?;
    let video_subsystem = sdl.video()?;
    let window = video_subsystem
        .window("Vulkan 1.3 Tutorial", 800, 600)
        .position_centered()
        .resizable()
        .vulkan()
        .build()?;

    Ok((sdl, window))
}

struct Renderer;

impl Renderer {
    fn init(window: Window) -> Result<Self, Box<dyn std::error::Error>> {
        let entry = ash::Entry::linked();

        let app_info = vk::ApplicationInfo::default()
            .application_name(c"Vulkan 1.3 Tutorial")
            .api_version(vk::API_VERSION_1_3);

        let mut enabled_extension_names = vec![];
        let window_required_extensions: Vec<_> = window
            .vulkan_instance_extensions()?
            .into_iter()
            .map(|s| CString::new(s).unwrap())
            .collect();
        for name in &window_required_extensions {
            enabled_extension_names.push(name.as_ptr())
        }

        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&enabled_extension_names);

        let instance = unsafe { entry.create_instance(&create_info, None)? };

        let physical_devices: Vec<vk::PhysicalDevice> =
            unsafe { instance.enumerate_physical_devices()? };
        if LOG_ALL_AVAILABLE_DEVICES {
            let mut all_device_names = vec![];
            for device in &physical_devices {
                let mut device_properties = vk::PhysicalDeviceProperties2::default();
                unsafe {
                    instance.get_physical_device_properties2(*device, &mut device_properties)
                };
                let device_name = device_name_as_string(device_properties);
                all_device_names.push(device_name);
            }
            dbg!(&all_device_names);
        }

        // select physical device
        let physical_device = physical_devices[DISCRETE_NVIDIA_GPU_INDEX_ON_MY_LAPTOP];
        let device_name = {
            let mut device_properties = vk::PhysicalDeviceProperties2::default();
            unsafe {
                instance.get_physical_device_properties2(physical_device, &mut device_properties)
            };
            device_name_as_string(device_properties)
        };
        println!("Selected device: {device_name}");

        // graphics queue
        let queue_families =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        let queue_family = queue_families
            .iter()
            .position(|props| props.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .unwrap() as u32;
        let queue_priorities = [1.0];
        let queue_create_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&queue_priorities);
        let enabled_extension_names: Vec<_> = [vk::KHR_SWAPCHAIN_NAME]
            .iter()
            .map(|cstr| cstr.as_ptr())
            .collect();
        let mut enabled_1_2_features = vk::PhysicalDeviceVulkan12Features::default()
            .descriptor_indexing(true)
            .shader_sampled_image_array_non_uniform_indexing(true)
            .descriptor_binding_variable_descriptor_count(true)
            .runtime_descriptor_array(true)
            .buffer_device_address(true);
        let mut enabled_1_3_features = vk::PhysicalDeviceVulkan13Features::default()
            .synchronization2(true)
            .dynamic_rendering(true);
        let enabled_1_0_features = vk::PhysicalDeviceFeatures::default().sampler_anisotropy(true);
        let queue_create_infos = [queue_create_info];
        let device_create_info = vk::DeviceCreateInfo::default()
            .enabled_extension_names(&enabled_extension_names)
            .queue_create_infos(&queue_create_infos)
            .push_next(&mut enabled_1_2_features)
            .push_next(&mut enabled_1_3_features)
            .enabled_features(&enabled_1_0_features);
        let device = unsafe { instance.create_device(physical_device, &device_create_info, None) }?;
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        // vulkan memory allocator
        let allocator_create_info =
            vk_mem::AllocatorCreateInfo::new(&instance, &device, physical_device);
        let allocator = unsafe { vk_mem::Allocator::new(allocator_create_info)? };

        // SDL surface
        let surface_ext = ash::khr::surface::Instance::new(&entry, &instance);
        let surface = window.vulkan_create_surface(instance.handle())?;
        let surface_capabilities = unsafe {
            surface_ext.get_physical_device_surface_capabilities(physical_device, surface)?
        };
        let swapchain_extent = surface_capabilities.current_extent;

        // swapchain
        let image_format = vk::Format::B8G8R8A8_SRGB;
        let swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(surface_capabilities.min_image_count)
            .image_format(image_format)
            .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
            .image_extent(swapchain_extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .pre_transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(vk::PresentModeKHR::FIFO);
        let swapchain_device_ext = ash::khr::swapchain::Device::new(&instance, &device);
        let swapchain =
            unsafe { swapchain_device_ext.create_swapchain(&swapchain_create_info, None)? };
        let swapchain_images = unsafe { swapchain_device_ext.get_swapchain_images(swapchain)? };

        // depth attachment
        let mut depth_format: Option<vk::Format> = None;
        let possible_depth_formats = [
            vk::Format::D32_SFLOAT_S8_UINT,
            vk::Format::D24_UNORM_S8_UINT,
        ];
        for possible_depth_format in possible_depth_formats {
            let mut props = vk::FormatProperties2::default();
            unsafe {
                instance.get_physical_device_format_properties2(
                    physical_device,
                    possible_depth_format,
                    &mut props,
                )
            };
            if props
                .format_properties
                .optimal_tiling_features
                .contains(vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT)
            {
                depth_format = Some(possible_depth_format);
                break;
            }
        }
        let depth_format = depth_format.expect("no valid depth format");

        // create depth image
        let depth_image_create_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(depth_format)
            .extent(
                vk::Extent3D::default()
                    .width(swapchain_extent.width)
                    .height(swapchain_extent.height)
                    .depth(1),
            )
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let alloc_info = vk_mem::AllocationCreateInfo {
            flags: vk_mem::AllocationCreateFlags::DEDICATED_MEMORY,
            usage: vk_mem::MemoryUsage::Auto,
            ..Default::default()
        };
        let (depth_image, depth_image_allocation) =
            unsafe { allocator.create_image(&depth_image_create_info, &alloc_info)? };
        let depth_view_create_info = vk::ImageViewCreateInfo::default()
            .image(depth_image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(depth_format)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::DEPTH)
                    .level_count(1)
                    .layer_count(1),
            );
        let depth_image_view = unsafe { device.create_image_view(&depth_view_create_info, None)? };

        Ok(Self)
    }
}

fn device_name_as_string(props: vk::PhysicalDeviceProperties2) -> String {
    let device_name_bytes: Vec<u8> = props
        .properties
        .device_name
        .into_iter()
        .filter(|&i| i != 0)
        .map(|i| i as u8)
        .collect();

    String::from_utf8_lossy(&device_name_bytes).to_string()
}
