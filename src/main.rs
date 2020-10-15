
use winapi::um::winuser::*;
use winapi::um::libloaderapi::GetModuleHandleA;
use winapi::um::d3d12::*;
use winapi::shared::windef::{HWND};
use winapi::shared::minwindef::{UINT, WPARAM, LPARAM, LRESULT};
use winapi::Interface;
use bedrock as br;
use uninit::extension_traits::*;

#[repr(transparent)]
pub struct ComPtr<T>(std::ptr::NonNull<T>);
impl<T> From<*mut T> for ComPtr<T> { fn from(p: *mut T) -> Self { ComPtr(unsafe { std::ptr::NonNull::new_unchecked(p) }) } }
impl<T> Drop for ComPtr<T> {
    fn drop(&mut self) {
        unsafe {
            (*(self.0.as_ptr() as *mut winapi::um::unknwnbase::IUnknown)).Release();
        }
    }
}
impl<T> Clone for ComPtr<T> {
    fn clone(&self) -> Self {
        unsafe {
            (*(self.0.as_ptr() as *mut winapi::um::unknwnbase::IUnknown)).AddRef();
        }
        ComPtr(self.0)
    }
}
impl<T> std::ops::Deref for ComPtr<T> {
    type Target = T;
    fn deref(&self) -> &T { unsafe { self.0.as_ref() } }
}
impl<T> std::ops::DerefMut for ComPtr<T> {
    fn deref_mut(&mut self) -> &mut T { unsafe { self.0.as_mut() } }
}
impl<T> ComPtr<T> {
    pub fn as_ptr(&self) -> *mut T { self.0.as_ptr() }
}

fn hr_to_ioresult(hr: winapi::shared::winerror::HRESULT) -> std::io::Result<()> {
    if winapi::shared::winerror::FAILED(hr) { Err(std::io::Error::from_raw_os_error(hr)) } else { Ok(()) }
}

pub struct UniqueObject<T: Copy, D: Fn(T)>(T, D);
impl<T: Copy, D: Fn(T)> Drop for UniqueObject<T, D> {
    fn drop(&mut self) {
        (self.1)(self.0);
    }
}
impl<T: Copy, D: Fn(T)> AsRef<T> for UniqueObject<T, D> {
    fn as_ref(&self) -> &T { &self.0 }
}
impl<T, D: Fn(*mut T)> UniqueObject<*mut T, D> {
    fn as_ptr(&self) -> *mut T { self.0 }
}
fn vk_to_result(r: br::vk::VkResult) -> Result<(), br::VkResultBox> {
    br::VkResultHandler::into_result(r)
}

fn main() {
    let wce = WNDCLASSEXA {
        cbSize: std::mem::size_of::<WNDCLASSEXA>() as _,
        lpszClassName: b"jp.ct2.experimental.vkNoRedirectRender\0".as_ptr() as _,
        lpfnWndProc: Some(wcb),
        hInstance: unsafe { GetModuleHandleA(std::ptr::null_mut()) },
        .. unsafe { std::mem::MaybeUninit::zeroed().assume_init() }
    };
    if unsafe { RegisterClassExA(&wce) == 0 } {
        panic!("RegisterClassEx failed: {:?}", std::io::Error::last_os_error());
    }

    let w = unsafe {
        CreateWindowExA(
            WS_EX_APPWINDOW | WS_EX_OVERLAPPEDWINDOW | WS_EX_NOREDIRECTIONBITMAP,
            wce.lpszClassName, b"vkNoRedirectRender\0".as_ptr() as _,
            WS_OVERLAPPEDWINDOW | WS_VISIBLE, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
            std::ptr::null_mut(), std::ptr::null_mut(), wce.hInstance, std::ptr::null_mut()
        )
    };
    if w.is_null() {
        panic!("CreateWindowEx failed: {:?}", std::io::Error::last_os_error());
    }

    // Initialize DXGI
    let mut factory = std::ptr::null_mut();
    let hr = unsafe { winapi::shared::dxgi1_3::CreateDXGIFactory2(winapi::shared::dxgi1_3::DXGI_CREATE_FACTORY_DEBUG, &winapi::shared::dxgi1_2::IDXGIFactory2::uuidof(), &mut factory) };
    hr_to_ioresult(hr).expect("CreateDXGIFactory2 failed");
    let factory = ComPtr::from(factory as *mut winapi::shared::dxgi1_2::IDXGIFactory2);
    let mut adapter = std::ptr::null_mut();
    let hr = unsafe { factory.EnumAdapters1(0, &mut adapter) };
    hr_to_ioresult(hr).expect("IDXGIAdapter1 Enumeration failed");
    let adapter = ComPtr::from(adapter);

    // Initialize Direct3D12
    let mut dbg = std::ptr::null_mut();
    let hr = unsafe { D3D12GetDebugInterface(&winapi::um::d3d12sdklayers::ID3D12Debug::uuidof(), &mut dbg) };
    hr_to_ioresult(hr).expect("D3D12GetDebugInterface failed");
    unsafe { ComPtr::from(dbg as *mut winapi::um::d3d12sdklayers::ID3D12Debug).EnableDebugLayer(); }

    let mut device12 = std::ptr::null_mut();
    let hr = unsafe { D3D12CreateDevice(adapter.as_ptr() as _, winapi::um::d3dcommon::D3D_FEATURE_LEVEL_12_0, &winapi::um::d3d12::ID3D12Device::uuidof(), &mut device12) };
    hr_to_ioresult(hr).expect("D3D12CreateDevice failed");
    let device12 = ComPtr::from(device12 as *mut winapi::um::d3d12::ID3D12Device);
    let cqdesc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        .. unsafe { std::mem::MaybeUninit::zeroed().assume_init() }
    };
    let mut cq = std::ptr::null_mut();
    let hr = unsafe { device12.CreateCommandQueue(&cqdesc, &winapi::um::d3d12::ID3D12CommandQueue::uuidof(), &mut cq) };
    hr_to_ioresult(hr).expect("D3D12 CreateCommandQueue failed");
    let cq = ComPtr::from(cq as *mut ID3D12CommandQueue);

    // Initialize SwapChain
    let scdesc = winapi::shared::dxgi1_2::DXGI_SWAP_CHAIN_DESC1 {
        Width: 640, Height: 480, Format: winapi::shared::dxgiformat::DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: winapi::shared::dxgitype::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferCount: 2, BufferUsage: winapi::shared::dxgitype::DXGI_USAGE_RENDER_TARGET_OUTPUT,
        Scaling: winapi::shared::dxgi1_2::DXGI_SCALING_STRETCH,
        SwapEffect: winapi::shared::dxgi::DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: winapi::shared::dxgi1_2::DXGI_ALPHA_MODE_PREMULTIPLIED,
        .. unsafe { std::mem::MaybeUninit::zeroed().assume_init() }
    };
    let mut sc = std::ptr::null_mut();
    let hr = unsafe { factory.CreateSwapChainForComposition(cq.as_ptr() as _, &scdesc, std::ptr::null_mut(), &mut sc) };
    hr_to_ioresult(hr).expect("DXGI CreateSwapChainForComposition failed");
    let sc = ComPtr::from(sc);

    // Initialize DirectComposition
    let mut comp_device = std::ptr::null_mut();
    let hr = unsafe { winapi::um::dcomp::DCompositionCreateDevice2(std::ptr::null(), &winapi::um::dcomp::IDCompositionDesktopDevice::uuidof(), &mut comp_device) };
    hr_to_ioresult(hr).expect("DCompositionCreateDevice2 failed");
    let comp_device = ComPtr::from(comp_device as *mut winapi::um::dcomp::IDCompositionDesktopDevice);
    let mut target = std::ptr::null_mut();
    let hr = unsafe { comp_device.CreateTargetForHwnd(w, 0, &mut target) };
    hr_to_ioresult(hr).expect("DComposition CreateTargetForHwnd failed");
    let target = ComPtr::from(target);
    let mut root = std::ptr::null_mut();
    let hr = unsafe { comp_device.CreateVisual(&mut root) };
    hr_to_ioresult(hr).expect("DComposition CreateVisual failed");
    let root = ComPtr::from(root);
    let hr = unsafe { root.SetContent(sc.as_ptr() as _) };
    hr_to_ioresult(hr).expect("DComposition SetContent for Visual failed");
    let hr = unsafe { target.SetRoot(root.as_ptr() as _) };
    hr_to_ioresult(hr).expect("DComposition SetRoot for Target failed");
    let hr = unsafe { comp_device.Commit() };
    hr_to_ioresult(hr).expect("DComposition Commit failed");

    // Initialize Vulkan
    let instance_layers = &[b"VK_LAYER_KHRONOS_validation\0".as_ptr() as _];
    let instance_extensions = &[b"VK_EXT_debug_report\0".as_ptr() as _];
    let app_info = br::vk::VkApplicationInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_APPLICATION_INFO,
        pNext: std::ptr::null(),
        apiVersion: br::vk::VK_API_VERSION_1_1,
        pApplicationName: b"vkNoRedirectRender\0".as_ptr() as _,
        applicationVersion: br::VK_MAKE_VERSION!(0, 1, 0),
        pEngineName: b"RawRender\0".as_ptr() as _,
        engineVersion: br::VK_MAKE_VERSION!(0, 1, 0)
    };
    let instance_cinfo = br::vk::VkInstanceCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        pApplicationInfo: &app_info,
        ppEnabledLayerNames: instance_layers.as_ptr(),
        enabledLayerCount: instance_layers.len() as _,
        ppEnabledExtensionNames: instance_extensions.as_ptr(),
        enabledExtensionCount: instance_extensions.len() as _
    };
    let mut instance = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateInstance(&instance_cinfo, std::ptr::null(), &mut instance) };
    vk_to_result(r).expect("vkCreateInstance failed");
    let instance = UniqueObject(instance, |o| unsafe { br::vk::vkDestroyInstance(o, std::ptr::null()); });
    let dbg_cinfo = br::vk::VkDebugReportCallbackCreateInfoEXT {
        sType: br::vk::VK_STRUCTURE_TYPE_DEBUG_REPORT_CALLBACK_CREATE_INFO_EXT,
        pNext: std::ptr::null(),
        flags: br::vk::VK_DEBUG_REPORT_ERROR_BIT_EXT | br::vk::VK_DEBUG_REPORT_WARNING_BIT_EXT,
        pfnCallback: vkcb,
        pUserData: std::ptr::null_mut()
    };
    let mut dbg = br::vk::VK_NULL_HANDLE as _;
    let ccb_ext_fn: br::vk::PFN_vkCreateDebugReportCallbackEXT = unsafe {
        std::mem::transmute(
            br::vk::vkGetInstanceProcAddr(instance.as_ptr(), b"vkCreateDebugReportCallbackEXT\0".as_ptr() as _)
                .expect("vkCreateDebugReportCallbackEXT not found?")
        )
    };
    let dcb_ext_fn: br::vk::PFN_vkDestroyDebugReportCallbackEXT = unsafe {
        std::mem::transmute(
            br::vk::vkGetInstanceProcAddr(instance.as_ptr(), b"vkDestroyDebugReportCallbackEXT\0".as_ptr() as _)
                .expect("vkDestroyDebugReportCallbackEXT not found?")
        )
    };
    let r = (ccb_ext_fn)(instance.as_ptr(), &dbg_cinfo, std::ptr::null(), &mut dbg);
    vk_to_result(r).expect("vkCreateDebugReportCallback failed");
    let _dbg = UniqueObject(dbg, |p| (dcb_ext_fn)(instance.as_ptr(), p, std::ptr::null()));
    let mut adapters = vec![br::vk::VK_NULL_HANDLE as _];
    let mut adapter_count = 1;
    let r = unsafe { br::vk::vkEnumeratePhysicalDevices(instance.as_ptr(), &mut adapter_count, adapters.as_mut_ptr()) };
    vk_to_result(r).expect("vkEnumeratePhysicalDevices failed");
    let vk_adapter = adapters.pop().expect("invalid");
    let mut queue_family_property_count = 0;
    unsafe { br::vk::vkGetPhysicalDeviceQueueFamilyProperties(vk_adapter, &mut queue_family_property_count, std::ptr::null_mut()) };
    let mut queue_family_properties = Vec::new();
    unsafe {
        br::vk::vkGetPhysicalDeviceQueueFamilyProperties(
            vk_adapter, &mut queue_family_property_count,
            queue_family_properties.reserve_uninit(queue_family_property_count as _).as_mut_ptr()
        )
    };
    unsafe { queue_family_properties.set_len(queue_family_properties.len() + queue_family_property_count as usize); }
    let queue_family_index = queue_family_properties.iter()
        .position(|p| p.queueCount > 0 && (p.queueFlags & br::vk::VK_QUEUE_GRAPHICS_BIT) != 0)
        .expect("no graphics queue?");
    let queue_priorities = &[0.0];
    let queue_create_info = br::vk::VkDeviceQueueCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        queueFamilyIndex: queue_family_index as _,
        queueCount: 1,
        pQueuePriorities: queue_priorities.as_ptr()
    };
    let device_extensions = &[b"VK_KHR_external_memory_win32\0".as_ptr() as _];
    let device_cinfo = br::vk::VkDeviceCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        ppEnabledLayerNames: std::ptr::null(),
        enabledLayerCount: 0,
        ppEnabledExtensionNames: device_extensions.as_ptr(),
        enabledExtensionCount: device_extensions.len() as _,
        pQueueCreateInfos: &queue_create_info,
        queueCreateInfoCount: 1,
        pEnabledFeatures: std::ptr::null()
    };
    let mut vk_device = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateDevice(vk_adapter, &device_cinfo, std::ptr::null(), &mut vk_device) };
    vk_to_result(r).expect("vkCreateDevice failed");
    let vk_device = UniqueObject(vk_device, |p| unsafe { br::vk::vkDestroyDevice(p, std::ptr::null()); });
    let mut vk_queue = br::vk::VK_NULL_HANDLE as _;
    unsafe { br::vk::vkGetDeviceQueue(vk_device.as_ptr(), queue_family_index as _, 0, &mut vk_queue) };

    let rp_attachment_desc = &[br::vk::VkAttachmentDescription {
        format: br::vk::VK_FORMAT_R8G8B8A8_UNORM,
        samples: br::vk::VK_SAMPLE_COUNT_1_BIT,
        loadOp: br::vk::VK_ATTACHMENT_LOAD_OP_CLEAR,
        storeOp: br::vk::VK_ATTACHMENT_STORE_OP_STORE,
        stencilLoadOp: br::vk::VK_ATTACHMENT_LOAD_OP_DONT_CARE,
        stencilStoreOp: br::vk::VK_ATTACHMENT_STORE_OP_DONT_CARE,
        initialLayout: br::vk::VK_IMAGE_LAYOUT_GENERAL,
        finalLayout: br::vk::VK_IMAGE_LAYOUT_GENERAL,
        flags: 0
    }];
    let rp_attachment_color_out = &[br::vk::VkAttachmentReference { attachment: 0, layout: br::vk::VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL }];
    let rp_subpass_color_desc = &[br::vk::VkSubpassDescription {
        flags: 0,
        pipelineBindPoint: br::vk::VK_PIPELINE_BIND_POINT_GRAPHICS,
        inputAttachmentCount: 0,
        pInputAttachments: std::ptr::null(),
        colorAttachmentCount: 1,
        pColorAttachments: rp_attachment_color_out.as_ptr(),
        pResolveAttachments: std::ptr::null(),
        pDepthStencilAttachment: std::ptr::null(),
        preserveAttachmentCount: 0,
        pPreserveAttachments: std::ptr::null()
    }];
    let rp_subpass_deps = &[br::vk::VkSubpassDependency {
        srcSubpass: 0, dstSubpass: 0,
        srcAccessMask: br::vk::VK_ACCESS_MEMORY_READ_BIT, dstAccessMask: br::vk::VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
        srcStageMask: br::vk::VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT, dstStageMask: br::vk::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
        dependencyFlags: br::vk::VK_DEPENDENCY_BY_REGION_BIT
    }];
    let rp_cinfo = br::vk::VkRenderPassCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        attachmentCount: 1,
        pAttachments: rp_attachment_desc.as_ptr(),
        subpassCount: 1,
        pSubpasses: rp_subpass_color_desc.as_ptr(),
        dependencyCount: 1,
        pDependencies: rp_subpass_deps.as_ptr()
    };
    let mut rp = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateRenderPass(vk_device.as_ptr(), &rp_cinfo, std::ptr::null(), &mut rp) };
    vk_to_result(r).expect("vkCreateRenderPass failed");
    let rp = UniqueObject(rp, |p| unsafe { br::vk::vkDestroyRenderPass(vk_device.as_ptr(), p, std::ptr::null()); });

    // Create Shared Object from Swapchain Backbuffers
    let mut memory_properties = std::mem::MaybeUninit::uninit();
    unsafe { br::vk::vkGetPhysicalDeviceMemoryProperties(vk_adapter, memory_properties.as_mut_ptr()) };
    let memory_properties = unsafe { memory_properties.assume_init() };

    let vk_get_memory_win32_handle_properties_khr: br::vk::PFN_vkGetMemoryWin32HandlePropertiesKHR = unsafe {
        std::mem::transmute(
            br::vk::vkGetDeviceProcAddr(vk_device.as_ptr(), b"vkGetMemoryWin32HandlePropertiesKHR\0".as_ptr() as _)
                .expect("vkGetMemoryWin32HandlePropertiesKHR not found?")
        )
    };
    let vk_backbuffer_resources = (0..2).map(|n| {
        let mut res = std::ptr::null_mut();
        let hr = unsafe { sc.GetBuffer(n as _, &winapi::um::d3d12::ID3D12Resource::uuidof(), &mut res) };
        hr_to_ioresult(hr).expect("SwapChain GetBuffer failed");
        let res = ComPtr::from(res as *mut winapi::um::d3d12::ID3D12Resource);
        let mut sh = std::ptr::null_mut();
        let name = widestring::WideCString::from_str(format!("LocalSharedBackBufferResource{}", n)).expect("WideCString encoding failed");
        let hr = unsafe { device12.CreateSharedHandle(res.as_ptr() as _, std::ptr::null(), winapi::um::winnt::GENERIC_ALL, name.as_ptr(), &mut sh) };
        hr_to_ioresult(hr).expect("D3D12 CreateSharedHandle failed");
        let sh = UniqueObject(sh, |p| unsafe { winapi::um::handleapi::CloseHandle(p); });

        let image_extmem_info = br::vk::VkExternalMemoryImageCreateInfo {
            sType: br::vk::VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
            pNext: std::ptr::null(),
            handleTypes: br::vk::VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT
        };
        let image_cinfo = br::vk::VkImageCreateInfo {
            sType: br::vk::VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
            pNext: &image_extmem_info as *const _ as _,
            imageType: br::vk::VK_IMAGE_TYPE_2D,
            format: br::vk::VK_FORMAT_R8G8B8A8_UNORM,
            extent: br::vk::VkExtent3D { width: 640, height: 480, depth: 1 },
            mipLevels: 1,
            arrayLayers: 1,
            samples: br::vk::VK_SAMPLE_COUNT_1_BIT,
            tiling: br::vk::VK_IMAGE_TILING_OPTIMAL,
            usage: br::vk::VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT,
            sharingMode: br::vk::VK_SHARING_MODE_EXCLUSIVE,
            queueFamilyIndexCount: 0,
            pQueueFamilyIndices: std::ptr::null(),
            initialLayout: br::vk::VK_IMAGE_LAYOUT_PREINITIALIZED,
            flags: 0
        };
        let mut image = br::vk::VK_NULL_HANDLE as _;
        let r = unsafe { br::vk::vkCreateImage(vk_device.as_ptr(), &image_cinfo, std::ptr::null(), &mut image) };
        vk_to_result(r).expect("vkCreateImage failed");
        let image = UniqueObject(image, |o| unsafe { br::vk::vkDestroyImage(vk_device.as_ptr(), o, std::ptr::null()); });
        let mut img_requirements = std::mem::MaybeUninit::uninit();
        unsafe { br::vk::vkGetImageMemoryRequirements(vk_device.as_ptr(), image.as_ptr(), img_requirements.as_mut_ptr()) };
        let img_requirements = unsafe { img_requirements.assume_init() };

        let mut props = br::vk::VkMemoryWin32HandlePropertiesKHR {
            sType: br::vk::VK_STRUCTURE_TYPE_MEMORY_WIN32_HANDLE_PROPERTIES_KHR,
            pNext: std::ptr::null_mut(),
            .. unsafe { std::mem::MaybeUninit::uninit().assume_init() }
        };
        let r = (vk_get_memory_win32_handle_properties_khr)(vk_device.as_ptr(), br::vk::VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT, sh.as_ptr(), &mut props);
        vk_to_result(r).expect("vkGetMemoryWin32HandlePropertiesKHR failed");
        let memory_type_index = memory_properties.memoryTypes[..memory_properties.memoryTypeCount as usize].iter().enumerate()
            .position(|(n, t)|
                (props.memoryTypeBits & (1 << n)) != 0 &&
                (img_requirements.memoryTypeBits & (1 << n)) != 0 &&
                (t.propertyFlags & br::vk::VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) != 0
            ).expect("no matching memory type index");
        let import_memory_info = br::vk::VkImportMemoryWin32HandleInfoKHR {
            sType: br::vk::VK_STRUCTURE_TYPE_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR,
            pNext: std::ptr::null(),
            handleType: br::vk::VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT,
            handle: sh.as_ptr(),
            name: name.as_ptr()
        };
        let memory_ainfo = br::vk::VkMemoryAllocateInfo {
            sType: br::vk::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            pNext: &import_memory_info as *const _ as _,
            allocationSize: img_requirements.size,  // ignored
            memoryTypeIndex: memory_type_index as _
        };
        let mut mem = br::vk::VK_NULL_HANDLE as _;
        let r = unsafe { br::vk::vkAllocateMemory(vk_device.as_ptr(), &memory_ainfo, std::ptr::null(), &mut mem) };
        vk_to_result(r).expect("vkAllocateMemory failed");
        let mem = UniqueObject(mem, |p| unsafe { br::vk::vkFreeMemory(vk_device.as_ptr(), p, std::ptr::null()); });
        let r = unsafe { br::vk::vkBindImageMemory(vk_device.as_ptr(), image.as_ptr(), mem.as_ptr(), 0) };
        vk_to_result(r).expect("vkBindImageMemory failed");

        let iv_cinfo = br::vk::VkImageViewCreateInfo {
            sType: br::vk::VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
            pNext: std::ptr::null(),
            image: image.as_ptr(),
            viewType: br::vk::VK_IMAGE_VIEW_TYPE_2D,
            format: image_cinfo.format,
            components: br::vk::VkComponentMapping {
                r: br::vk::VK_COMPONENT_SWIZZLE_R,
                g: br::vk::VK_COMPONENT_SWIZZLE_G,
                b: br::vk::VK_COMPONENT_SWIZZLE_B,
                a: br::vk::VK_COMPONENT_SWIZZLE_A
            },
            subresourceRange: br::vk::VkImageSubresourceRange {
                aspectMask: br::vk::VK_IMAGE_ASPECT_COLOR_BIT,
                baseMipLevel: 0,
                levelCount: 1,
                baseArrayLayer: 0,
                layerCount: 1
            },
            flags: 0
        };
        let mut iv = br::vk::VK_NULL_HANDLE as _;
        let r = unsafe { br::vk::vkCreateImageView(vk_device.as_ptr(), &iv_cinfo, std::ptr::null(), &mut iv) };
        vk_to_result(r).expect("vkCreateImageView failed");
        let iv = UniqueObject(iv, |p| unsafe { br::vk::vkDestroyImageView(vk_device.as_ptr(), p, std::ptr::null()); });

        let image_views = &[iv.as_ptr()];
        let fb_cinfo = br::vk::VkFramebufferCreateInfo {
            sType: br::vk::VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
            pNext: std::ptr::null(),
            flags: 0,
            renderPass: rp.as_ptr(),
            attachmentCount: 1,
            pAttachments: image_views.as_ptr(),
            width: 640,
            height: 480,
            layers: 1
        };
        let mut fb = br::vk::VK_NULL_HANDLE as _;
        let r = unsafe { br::vk::vkCreateFramebuffer(vk_device.as_ptr(), &fb_cinfo, std::ptr::null(), &mut fb) };
        vk_to_result(r).expect("vkCreateFramebuffer failed");
        let fb = UniqueObject(fb, |p| unsafe { br::vk::vkDestroyFramebuffer(vk_device.as_ptr(), p, std::ptr::null()); });

        (sh, mem, image, iv, fb)
    }).collect::<Vec<_>>();

    let mut msg = unsafe { std::mem::MaybeUninit::uninit().assume_init() };
    while unsafe { GetMessageA(&mut msg, std::ptr::null_mut(), 0, 0) > 0 } {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
    }
}

extern "system" fn wcb(hwnd: HWND, msg: UINT, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => unsafe { PostQuitMessage(0); return 0; },
        _ => ()
    }

    unsafe { DefWindowProcA(hwnd, msg, wp, lp) }
}

extern "system" fn vkcb(
    flags: br::vk::VkDebugReportFlagsEXT,
    _: br::vk::VkDebugReportObjectTypeEXT,
    _: u64,
    _: libc::size_t,
    _: i32,
    layer_prefix: *const libc::c_char,
    message: *const libc::c_char,
    _: *mut libc::c_void
) -> br::vk::VkBool32 {
    println!(
        "vkcb: [{}] {}",
        unsafe { std::ffi::CStr::from_ptr(layer_prefix).to_string_lossy() },
        unsafe { std::ffi::CStr::from_ptr(message).to_string_lossy() }
    );

    ((flags & br::vk::VK_DEBUG_REPORT_ERROR_BIT_EXT) != 0) as _
}
