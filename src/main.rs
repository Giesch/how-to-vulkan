use std::ffi::CString;

use ash::vk;
use sdl3::video::Window;

fn main() {
    todo!();
}

struct Renderer;

impl Renderer {
    fn init(window: Window) -> Result<Self, Box<dyn std::error::Error>> {
        let entry = ash::Entry::linked();

        let app_info = vk::ApplicationInfo::default()
            .application_name(c"Vulkan Tutorial")
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

        Ok(Self)
    }
}
