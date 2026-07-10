#![allow(unsafe_code, unused_variables)]

use std::ffi::CString;
use std::path::PathBuf;

use ash::vk;
use glam::{Mat4, Vec2, Vec3, Vec4};
use sdl3::Sdl;
use sdl3::video::Window;
use vk_mem::Alloc;

const MAX_FRAMES_IN_FLIGHT: usize = 2;
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

struct Renderer {
    device: ash::Device,
    allocator: vk_mem::Allocator,
    depth_image: vk::Image,
    depth_image_view: vk::ImageView,
    depth_image_allocation: vk_mem::Allocation,
    vi_buffer: vk::Buffer,
    vi_buffer_allocation: vk_mem::Allocation,
    shader_data_buffers: [ShaderDataBuffer; MAX_FRAMES_IN_FLIGHT],
    fences: [vk::Fence; MAX_FRAMES_IN_FLIGHT],
    image_acquired_semaphores: [vk::Semaphore; MAX_FRAMES_IN_FLIGHT],
    render_complete_semaphores: [vk::Semaphore; MAX_FRAMES_IN_FLIGHT],
    // command_buffers: [vk::CommandBuffer; MAX_FRAMES_IN_FLIGHT],
}

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
        // https://www.howtovulkan.com/#device-selection
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
        // https://www.howtovulkan.com/#queues
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
        // https://www.howtovulkan.com/#setting-up-vma
        let mut allocator_create_info =
            vk_mem::AllocatorCreateInfo::new(&instance, &device, physical_device);
        allocator_create_info.flags = vk_mem::AllocatorCreateFlags::BUFFER_DEVICE_ADDRESS;
        let allocator = unsafe { vk_mem::Allocator::new(allocator_create_info)? };

        // SDL surface
        // https://www.howtovulkan.com/#window-and-surface
        let surface_ext = ash::khr::surface::Instance::new(&entry, &instance);
        let surface = window.vulkan_create_surface(instance.handle())?;
        let surface_capabilities = unsafe {
            surface_ext.get_physical_device_surface_capabilities(physical_device, surface)?
        };
        let swapchain_extent = surface_capabilities.current_extent;

        // swapchain
        // https://www.howtovulkan.com/#swapchain
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
        // https://www.howtovulkan.com/#depth-attachment
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
            let device_supports_depth_format = props
                .format_properties
                .optimal_tiling_features
                .contains(vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT);
            if device_supports_depth_format {
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
        let depth_alloc_info = vk_mem::AllocationCreateInfo {
            flags: vk_mem::AllocationCreateFlags::DEDICATED_MEMORY,
            usage: vk_mem::MemoryUsage::Auto,
            ..Default::default()
        };
        let (depth_image, depth_image_allocation) =
            unsafe { allocator.create_image(&depth_image_create_info, &depth_alloc_info)? };
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

        // vertex/index buffer
        // https://www.howtovulkan.com/#loading-meshes
        let (verticies, indicies) = load_obj()?;
        // NOTE the as_slice() calls here are necessary to avoid counting the vec header
        let verts_size = std::mem::size_of_val(verticies.as_slice());
        let indicies_size = std::mem::size_of_val(indicies.as_slice());
        let buffer_size = verts_size + indicies_size;
        let buffer_create_info = vk::BufferCreateInfo::default()
            .size(buffer_size as u64)
            .usage(vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let vert_alloc_create_info = vk_mem::AllocationCreateInfo {
            flags: vk_mem::AllocationCreateFlags::HOST_ACCESS_SEQUENTIAL_WRITE
                | vk_mem::AllocationCreateFlags::HOST_ACCESS_ALLOW_TRANSFER_INSTEAD
                | vk_mem::AllocationCreateFlags::MAPPED,
            usage: vk_mem::MemoryUsage::Auto,
            ..Default::default()
        };
        let (vi_buffer, vi_buffer_allocation) =
            unsafe { allocator.create_buffer(&buffer_create_info, &vert_alloc_create_info)? };
        let mapped_vi_buffer = allocator
            .get_allocation_info(&vi_buffer_allocation)
            .mapped_data;
        unsafe {
            std::slice::from_raw_parts_mut(mapped_vi_buffer as *mut Vertex, verticies.len())
                .copy_from_slice(&verticies);
            let indicies_start = mapped_vi_buffer.add(verts_size) as *mut u32;
            std::slice::from_raw_parts_mut(indicies_start, indicies.len())
                .copy_from_slice(&indicies);
        }

        let mut shader_data_buffers = vec![];
        for f in 0..MAX_FRAMES_IN_FLIGHT {
            let buffer_create_info = vk::BufferCreateInfo::default()
                .size(size_of::<ShaderData>() as u64)
                .usage(vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS);
            let alloc_create_info = vk_mem::AllocationCreateInfo {
                flags: vk_mem::AllocationCreateFlags::HOST_ACCESS_SEQUENTIAL_WRITE
                    | vk_mem::AllocationCreateFlags::HOST_ACCESS_ALLOW_TRANSFER_INSTEAD
                    | vk_mem::AllocationCreateFlags::MAPPED,
                usage: vk_mem::MemoryUsage::Auto,
                ..Default::default()
            };
            let (buffer, allocation) =
                unsafe { allocator.create_buffer(&buffer_create_info, &alloc_create_info)? };

            let bda_info = vk::BufferDeviceAddressInfo::default().buffer(buffer);
            let device_address = unsafe { device.get_buffer_device_address(&bda_info) };

            shader_data_buffers.push(ShaderDataBuffer {
                buffer,
                allocation,
                device_address,
            });
        }
        let shader_data_buffers: [ShaderDataBuffer; MAX_FRAMES_IN_FLIGHT] =
            shader_data_buffers.try_into().unwrap();

        // https://www.howtovulkan.com/#synchronization-objects
        let semaphore_create_info = vk::SemaphoreCreateInfo::default();
        let fence_create_info =
            vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let mut fences = vec![];
        let mut image_acquired_semaphores = vec![];
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            let semaphore = unsafe { device.create_semaphore(&semaphore_create_info, None)? };
            let fence = unsafe { device.create_fence(&fence_create_info, None)? };
            fences.push(fence);
            image_acquired_semaphores.push(semaphore);
        }
        let mut render_complete_semaphores = vec![];
        for _ in 0..render_complete_semaphores.len() {
            let semaphore = unsafe { device.create_semaphore(&semaphore_create_info, None)? };
            render_complete_semaphores.push(semaphore);
        }
        let fences: [vk::Fence; MAX_FRAMES_IN_FLIGHT] = fences.try_into().unwrap();
        let image_acquired_semaphores: [vk::Semaphore; MAX_FRAMES_IN_FLIGHT] =
            image_acquired_semaphores.try_into().unwrap();
        let render_complete_semaphores: [vk::Semaphore; MAX_FRAMES_IN_FLIGHT] =
            render_complete_semaphores.try_into().unwrap();

        // let command_buffers: [vk::CommandBuffer; MAX_FRAMES_IN_FLIGHT];

        Ok(Self {
            device,
            allocator,
            depth_image,
            depth_image_view,
            depth_image_allocation,
            vi_buffer,
            vi_buffer_allocation,
            shader_data_buffers,
            fences,
            image_acquired_semaphores,
            render_complete_semaphores,
            // command_buffers,
        })
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_image_view(self.depth_image_view, None);
            self.allocator
                .destroy_image(self.depth_image, &mut self.depth_image_allocation);

            self.allocator
                .destroy_buffer(self.vi_buffer, &mut self.vi_buffer_allocation);

            for shader_data_buffer in &mut self.shader_data_buffers {
                self.allocator.destroy_buffer(
                    shader_data_buffer.buffer,
                    &mut shader_data_buffer.allocation,
                );
            }
        }
    }
}

#[derive(Debug)]
struct ShaderDataBuffer {
    buffer: vk::Buffer,
    allocation: vk_mem::Allocation,
    device_address: vk::DeviceAddress,
}

#[repr(C, align(16))]
struct ShaderData {
    projection: Mat4,
    view: Mat4,
    model: [Mat4; 3],
    // 0.0, -10.0, 10.0, 0.0
    light_pos: Vec4,
    selected: usize,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Vertex {
    position: Vec3,
    normal: Vec3,
    uv: Vec2,
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

// From unknownue's rust version of the original vulkan tutorial
// https://github.com/unknownue/vulkan-tutorial-rust/blob/master/src/tutorials/27_model_loading.rs
fn load_obj() -> Result<(Vec<Vertex>, Vec<u32>), tobj::LoadError> {
    let file_path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "assets", "suzanne.obj"]
        .iter()
        .collect();

    let (mut models, _materials) = tobj::load_obj(file_path, &tobj::GPU_LOAD_OPTIONS)?;

    debug_assert!(models.len() == 1);
    let model = models.remove(0);

    let mut vertices = vec![];
    let mesh = model.mesh;
    let vertices_count = mesh.positions.len() / 3;
    for i in 0..vertices_count {
        let vec3_offset = i * 3;
        let position = Vec3::new(
            mesh.positions[vec3_offset],
            mesh.positions[vec3_offset + 1],
            mesh.positions[vec3_offset + 2],
        );
        let normal = Vec3::new(
            mesh.normals[vec3_offset],
            mesh.normals[vec3_offset + 1],
            mesh.normals[vec3_offset + 2],
        );

        let uv = {
            let offset = i * 2;
            let u = mesh.texcoords[offset];
            // in obj, 0 is the bottom, in vulkan, 0 is the top
            // (for texture coordinates)
            let v = 1.0 - mesh.texcoords[offset + 1];
            Vec2::new(u, v)
        };

        let vertex = Vertex {
            position,
            normal,
            uv,
        };

        vertices.push(vertex);
    }

    Ok((vertices, mesh.indices))
}
