
use winapi::um::winuser::*;
use winapi::um::libloaderapi::GetModuleHandleA;
use winapi::um::d3d12::*;
use winapi::shared::windef::{HWND};
use winapi::shared::minwindef::{UINT, WPARAM, LPARAM, LRESULT};
use winapi::Interface;
use bedrock as br;
use uninit::extension_traits::*;
use std::io::prelude::Read;

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

#[repr(C)]
#[derive(Clone)]
pub struct Vertex { pub pos: [f32; 4], pub color: [f32; 4] }
#[repr(C)]
pub struct TimerUniform { pub time: f32 }

fn align2(x: usize, a: usize) -> usize { (x + (a - 1)) & !(a - 1) }

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
        Flags: winapi::shared::dxgi::DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
        .. unsafe { std::mem::MaybeUninit::zeroed().assume_init() }
    };
    let mut sc = std::ptr::null_mut();
    let hr = unsafe { factory.CreateSwapChainForComposition(cq.as_ptr() as _, &scdesc, std::ptr::null_mut(), &mut sc) };
    hr_to_ioresult(hr).expect("DXGI CreateSwapChainForComposition failed");
    let sc = ComPtr::from(sc);
    let mut sc3 = std::ptr::null_mut();
    let hr = unsafe { sc.QueryInterface(&winapi::shared::dxgi1_4::IDXGISwapChain3::uuidof(), &mut sc3) };
    hr_to_ioresult(hr).expect("Querying IDXGISwapChain3 failed");
    let sc = ComPtr::from(sc3 as *mut winapi::shared::dxgi1_4::IDXGISwapChain3);
    let sc_waitable = unsafe { sc.GetFrameLatencyWaitableObject() };
    let mut fence = std::ptr::null_mut();
    let hr = unsafe { device12.CreateFence(0, D3D12_FENCE_FLAG_NONE, &ID3D12Fence::uuidof(), &mut fence) };
    hr_to_ioresult(hr).expect("D3D12 CreateFence failed");
    let fence12 = ComPtr::from(fence as *mut ID3D12Fence);

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

    let mut memory_properties = std::mem::MaybeUninit::uninit();
    unsafe { br::vk::vkGetPhysicalDeviceMemoryProperties(vk_adapter, memory_properties.as_mut_ptr()) };
    let memory_properties = unsafe { memory_properties.assume_init() };

    // Initialize Vulkan Rendering
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

    let buf_offset_vertices = align2(std::mem::size_of::<TimerUniform>(), 16);
    let buf_size = buf_offset_vertices + std::mem::size_of::<[Vertex; 3]>();
    let mut buffer_cinfo = br::vk::VkBufferCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        size: buf_size as _,
        usage: br::vk::VK_BUFFER_USAGE_VERTEX_BUFFER_BIT | br::vk::VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT | br::vk::VK_BUFFER_USAGE_TRANSFER_DST_BIT,
        sharingMode: br::vk::VK_SHARING_MODE_EXCLUSIVE,
        queueFamilyIndexCount: 0,
        pQueueFamilyIndices: std::ptr::null()
    };
    let mut buffer = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateBuffer(vk_device.as_ptr(), &buffer_cinfo, std::ptr::null(), &mut buffer) };
    vk_to_result(r).expect("vkCreateBuffer failed");
    let buffer = UniqueObject(buffer, |p| unsafe { br::vk::vkDestroyBuffer(vk_device.as_ptr(), p, std::ptr::null()); });
    buffer_cinfo.usage = br::vk::VK_BUFFER_USAGE_TRANSFER_SRC_BIT;
    let mut stg_buffer = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateBuffer(vk_device.as_ptr(), &buffer_cinfo, std::ptr::null(), &mut stg_buffer) };
    vk_to_result(r).expect("vkCreateBuffer for Staging failed");
    let stg_buffer = UniqueObject(stg_buffer, |p| unsafe { br::vk::vkDestroyBuffer(vk_device.as_ptr(), p, std::ptr::null()); });
    let mut buffer_memreq = std::mem::MaybeUninit::uninit();
    unsafe { br::vk::vkGetBufferMemoryRequirements(vk_device.as_ptr(), buffer.as_ptr(), buffer_memreq.as_mut_ptr()) };
    let buffer_memreq = unsafe { buffer_memreq.assume_init() };
    let buffer_mem_ainfo = br::vk::VkMemoryAllocateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        pNext: std::ptr::null(),
        allocationSize: buffer_memreq.size,
        memoryTypeIndex: memory_properties.memoryTypes[..memory_properties.memoryTypeCount as usize].iter().enumerate()
            .position(|(n, t)| (buffer_memreq.memoryTypeBits & (1 << n)) != 0 && (t.propertyFlags & br::vk::VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) != 0)
            .expect("device local memory not found?") as _
    };
    let mut buffer_mem = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkAllocateMemory(vk_device.as_ptr(), &buffer_mem_ainfo, std::ptr::null(), &mut buffer_mem) };
    vk_to_result(r).expect("vkAllocateMemory failed");
    let buffer_mem = UniqueObject(buffer_mem, |p| unsafe { br::vk::vkFreeMemory(vk_device.as_ptr(), p, std::ptr::null()); });
    let r = unsafe { br::vk::vkBindBufferMemory(vk_device.as_ptr(), buffer.as_ptr(), buffer_mem.as_ptr(), 0) };
    vk_to_result(r).expect("vkBindBufferMemory failed");
    let mut buffer_memreq = std::mem::MaybeUninit::uninit();
    unsafe { br::vk::vkGetBufferMemoryRequirements(vk_device.as_ptr(), stg_buffer.as_ptr(), buffer_memreq.as_mut_ptr()); };
    let buffer_memreq = unsafe { buffer_memreq.assume_init() };
    let stg_memory_type_index = memory_properties.memoryTypes[..memory_properties.memoryTypeCount as usize].iter().enumerate()
        .position(|(n, t)| (buffer_memreq.memoryTypeBits & (1 << n)) != 0 && (t.propertyFlags & br::vk::VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT) != 0)
        .expect("host visible memory not found?");
    let needs_stg_memory_cache_flush =
        (memory_properties.memoryTypes[stg_memory_type_index].propertyFlags & br::vk::VK_MEMORY_PROPERTY_HOST_CACHED_BIT) != 0 &&
        (memory_properties.memoryTypes[stg_memory_type_index].propertyFlags & br::vk::VK_MEMORY_PROPERTY_HOST_COHERENT_BIT) == 0;
    let buffer_mem_ainfo = br::vk::VkMemoryAllocateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        pNext: std::ptr::null(),
        allocationSize: buffer_memreq.size,
        memoryTypeIndex: stg_memory_type_index as _
    };
    let mut stg_buffer_mem = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkAllocateMemory(vk_device.as_ptr(), &buffer_mem_ainfo, std::ptr::null(), &mut stg_buffer_mem) };
    vk_to_result(r).expect("vkAllocateMemory for Staging failed");
    let stg_buffer_mem = UniqueObject(stg_buffer_mem, |p| unsafe { br::vk::vkFreeMemory(vk_device.as_ptr(), p, std::ptr::null()); });
    let r = unsafe { br::vk::vkBindBufferMemory(vk_device.as_ptr(), stg_buffer.as_ptr(), stg_buffer_mem.as_ptr(), 0) };
    vk_to_result(r).expect("vkBindBufferMemory for Staging failed");
    let mut p = std::ptr::null_mut();
    let r = unsafe { br::vk::vkMapMemory(vk_device.as_ptr(), stg_buffer_mem.as_ptr(), 0, buf_size as _, 0, &mut p) };
    vk_to_result(r).expect("vkMapMemory failed");
    let p = p as *mut u8;
    unsafe {
        *(p as *mut TimerUniform) = TimerUniform { time: 0.0 };
        std::slice::from_raw_parts_mut(p.add(buf_offset_vertices) as *mut Vertex, 3).clone_from_slice(&[
            Vertex { pos: [0.0, 0.5, 0.5, 1.0], color: [1.0, 1.0, 1.0, 0.6] },
            Vertex { pos: [0.5, -0.5, 0.5, 1.0], color: [0.0, 1.0, 1.0, 1.0] },
            Vertex { pos: [-0.5, -0.5, 0.5, 1.0], color: [1.0, 1.0, 0.0, 1.0] }
        ]);
    }
    if needs_stg_memory_cache_flush {
        let ranges = &[
            br::vk::VkMappedMemoryRange {
                sType: br::vk::VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE,
                pNext: std::ptr::null(),
                memory: stg_buffer_mem.as_ptr(),
                offset: 0,
                size: buf_size as _
            }
        ];
        let r = unsafe { br::vk::vkFlushMappedMemoryRanges(vk_device.as_ptr(), ranges.len() as _, ranges.as_ptr()) };
        vk_to_result(r).expect("vkFlushMappedMemoryRanges failed");
    }
    unsafe { br::vk::vkUnmapMemory(vk_device.as_ptr(), stg_buffer_mem.as_ptr()) };

    let dsl_ub1_v_bindings = &[br::vk::VkDescriptorSetLayoutBinding {
        binding: 0,
        descriptorType: br::vk::VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
        descriptorCount: 1,
        stageFlags: br::vk::VK_SHADER_STAGE_VERTEX_BIT,
        pImmutableSamplers: std::ptr::null()
    }];
    let dsl_ub1_v_cinfo = br::vk::VkDescriptorSetLayoutCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        bindingCount: 1,
        pBindings: dsl_ub1_v_bindings.as_ptr()
    };
    let mut dsl_ub1_v = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateDescriptorSetLayout(vk_device.as_ptr(), &dsl_ub1_v_cinfo, std::ptr::null(), &mut dsl_ub1_v) };
    vk_to_result(r).expect("vkCreateDescriptorSetLayout failed");
    let dsl_ub1_v = UniqueObject(dsl_ub1_v, |p| unsafe { br::vk::vkDestroyDescriptorSetLayout(vk_device.as_ptr(), p, std::ptr::null()); });
    let dsp_size = &[br::vk::VkDescriptorPoolSize { _type: br::vk::VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, descriptorCount: 1 }];
    let dsp_cinfo = br::vk::VkDescriptorPoolCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        poolSizeCount: 1,
        pPoolSizes: dsp_size.as_ptr(),
        maxSets: 1
    };
    let mut dspool = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateDescriptorPool(vk_device.as_ptr(), &dsp_cinfo, std::ptr::null(), &mut dspool) };
    vk_to_result(r).expect("vkCreateDescriptorPool failed");
    let dspool = UniqueObject(dspool, |p| unsafe { br::vk::vkDestroyDescriptorPool(vk_device.as_ptr(), p, std::ptr::null()); });
    let dsp_alloc_layouts = &[dsl_ub1_v.as_ptr()];
    let dsp_ainfo = br::vk::VkDescriptorSetAllocateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
        pNext: std::ptr::null(),
        descriptorPool: dspool.as_ptr(),
        descriptorSetCount: 1,
        pSetLayouts: dsp_alloc_layouts.as_ptr()
    };
    let mut sets = vec![br::vk::VK_NULL_HANDLE as _; 1];
    let r = unsafe { br::vk::vkAllocateDescriptorSets(vk_device.as_ptr(), &dsp_ainfo, sets.as_mut_ptr()) };
    vk_to_result(r).expect("vkAllocateDescriptorSets failed");
    let ubinfo_timer = &[
        br::vk::VkDescriptorBufferInfo {
            buffer: buffer.as_ptr(), offset: 0, range: std::mem::size_of::<TimerUniform>() as _
        }
    ];
    let descriptor_writes = &[
        br::vk::VkWriteDescriptorSet {
            sType: br::vk::VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
            pNext: std::ptr::null(),
            dstSet: sets[0],
            dstBinding: 0,
            dstArrayElement: 0,
            descriptorType: br::vk::VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
            descriptorCount: 1,
            pBufferInfo: ubinfo_timer.as_ptr(),
            .. unsafe { std::mem::MaybeUninit::zeroed().assume_init() }
        }
    ];
    unsafe { br::vk::vkUpdateDescriptorSets(vk_device.as_ptr(), descriptor_writes.len() as _, descriptor_writes.as_ptr(), 0, std::ptr::null()) };

    let vert_binary = std::fs::File::open("./assets/vert.spv").and_then(|mut fp| {
        let binsize = fp.metadata()?.len() as usize;
        let bin = vec![0u32; (binsize + 3) / 4];
        let mut read_bytes = 0;
        while read_bytes < binsize {
            read_bytes += fp.read(unsafe { std::slice::from_raw_parts_mut((bin.as_ptr() as *mut u8).add(read_bytes), binsize - read_bytes) })?;
        }

        Ok(bin)
    }).expect("Vertex Shader loading failed");
    let frag_binary = std::fs::File::open("./assets/frag.spv").and_then(|mut fp| {
        let binsize = fp.metadata()?.len() as usize;
        let bin = vec![0u32; (binsize + 3) / 4];
        let mut read_bytes = 0;
        while read_bytes < binsize {
            read_bytes += fp.read(unsafe { std::slice::from_raw_parts_mut((bin.as_ptr() as *mut u8).add(read_bytes), binsize - read_bytes) })?;
        }

        Ok(bin)
    }).expect("Fragment Shader loading failed");
    let vert_shader_cinfo = br::vk::VkShaderModuleCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        codeSize: (vert_binary.len() * 4) as _,
        pCode: vert_binary.as_ptr()
    };
    let mut vert_shader = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateShaderModule(vk_device.as_ptr(), &vert_shader_cinfo, std::ptr::null(), &mut vert_shader) };
    vk_to_result(r).expect("vkCreateShaderModule Vertex failed");
    let vert_shader = UniqueObject(vert_shader, |p| unsafe { br::vk::vkDestroyShaderModule(vk_device.as_ptr(), p, std::ptr::null()); });
    let frag_shader_cinfo = br::vk::VkShaderModuleCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        codeSize: (frag_binary.len() * 4) as _,
        pCode: frag_binary.as_ptr()
    };
    let mut frag_shader = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateShaderModule(vk_device.as_ptr(), &frag_shader_cinfo, std::ptr::null(), &mut frag_shader) };
    vk_to_result(r).expect("vkCreateShaderModule Fragment failed");
    let frag_shader = UniqueObject(frag_shader, |p| unsafe { br::vk::vkDestroyShaderModule(vk_device.as_ptr(), p, std::ptr::null()); });
    let ps_layout_dsls = &[dsl_ub1_v.as_ptr()];
    let ps_layout_cinfo = br::vk::VkPipelineLayoutCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        setLayoutCount: ps_layout_dsls.len() as _,
        pSetLayouts: ps_layout_dsls.as_ptr() as _,
        pushConstantRangeCount: 0,
        pPushConstantRanges: std::ptr::null()
    };
    let mut ps_layout = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreatePipelineLayout(vk_device.as_ptr(), &ps_layout_cinfo, std::ptr::null(), &mut ps_layout) };
    vk_to_result(r).expect("vkCreatePipelineLayout failed");
    let ps_layout = UniqueObject(ps_layout, |p| unsafe { br::vk::vkDestroyPipelineLayout(vk_device.as_ptr(), p, std::ptr::null()); });
    let shader_entry = std::ffi::CString::new("main").expect("ffi encoding failed");
    let shader_stage_cinfos = &[
        br::vk::VkPipelineShaderStageCreateInfo {
            sType: br::vk::VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            pNext: std::ptr::null(),
            flags: 0,
            stage: br::vk::VK_SHADER_STAGE_VERTEX_BIT,
            module: vert_shader.as_ptr(),
            pName: shader_entry.as_ptr(),
            pSpecializationInfo: std::ptr::null()
        },
        br::vk::VkPipelineShaderStageCreateInfo {
            sType: br::vk::VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            pNext: std::ptr::null(),
            flags: 0,
            stage: br::vk::VK_SHADER_STAGE_FRAGMENT_BIT,
            module: frag_shader.as_ptr(),
            pName: shader_entry.as_ptr(),
            pSpecializationInfo: std::ptr::null()
        }
    ];
    let vertex_input_bindings = &[
        br::vk::VkVertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Vertex>() as _,
            inputRate: br::vk::VK_VERTEX_INPUT_RATE_VERTEX
        }
    ];
    let vertex_input_attributes = &[
        br::vk::VkVertexInputAttributeDescription {
            binding: 0,
            location: 0,
            offset: 0,
            format: br::vk::VK_FORMAT_R32G32B32A32_SFLOAT
        },
        br::vk::VkVertexInputAttributeDescription {
            binding: 0,
            location: 1,
            offset: std::mem::size_of::<[f32; 4]>() as _,
            format: br::vk::VK_FORMAT_R32G32B32A32_SFLOAT
        }
    ];
    let vertex_input_state_cinfo = br::vk::VkPipelineVertexInputStateCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STAGE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        vertexBindingDescriptionCount: vertex_input_bindings.len() as _,
        pVertexBindingDescriptions: vertex_input_bindings.as_ptr(),
        vertexAttributeDescriptionCount: vertex_input_attributes.len() as _,
        pVertexAttributeDescriptions: vertex_input_attributes.as_ptr()
    };
    let input_assembly_state_cinfo = br::vk::VkPipelineInputAssemblyStateCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        topology: br::vk::VK_PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP,
        primitiveRestartEnable: false as _
    };
    let viewports = &[
        br::vk::VkViewport {
            x: 0.0, y: 0.0, width: 640.0, height: 480.0, minDepth: 0.0, maxDepth: 1.0
        }
    ];
    let scissors = &[
        br::vk::VkRect2D {
            offset: br::vk::VkOffset2D { x: 0, y: 0 },
            extent: br::vk::VkExtent2D { width: 640, height: 480 }
        }
    ];
    let viewport_state_cinfo = br::vk::VkPipelineViewportStateCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        viewportCount: viewports.len() as _,
        pViewports: viewports.as_ptr(),
        scissorCount: scissors.len() as _,
        pScissors: scissors.as_ptr()
    };
    let rasterization_state_cinfo = br::vk::VkPipelineRasterizationStateCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        depthClampEnable: false as _,
        rasterizerDiscardEnable: false as _,
        polygonMode: br::vk::VK_POLYGON_MODE_FILL,
        cullMode: br::vk::VK_CULL_MODE_NONE,
        frontFace: br::vk::VK_FRONT_FACE_COUNTER_CLOCKWISE,
        depthBiasEnable: false as _,
        depthBiasConstantFactor: 0.0,
        depthBiasClamp: 0.0,
        depthBiasSlopeFactor: 0.0,
        lineWidth: 1.0
    };
    let multisample_state_cinfo = br::vk::VkPipelineMultisampleStateCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        rasterizationSamples: br::vk::VK_SAMPLE_COUNT_1_BIT,
        sampleShadingEnable: false as _,
        minSampleShading: 1.0,
        pSampleMask: std::ptr::null(),
        alphaToCoverageEnable: false as _,
        alphaToOneEnable: false as _
    };
    let color_blend_states = &[
        br::vk::VkPipelineColorBlendAttachmentState {
            blendEnable: true as _,
            srcColorBlendFactor: br::vk::VK_BLEND_FACTOR_ONE,
            dstColorBlendFactor: br::vk::VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
            colorBlendOp: br::vk::VK_BLEND_OP_ADD,
            srcAlphaBlendFactor: br::vk::VK_BLEND_FACTOR_ONE,
            dstAlphaBlendFactor: br::vk::VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
            alphaBlendOp: br::vk::VK_BLEND_OP_ADD,
            colorWriteMask: 0x0f
        }
    ];
    let blend_state_cinfo = br::vk::VkPipelineColorBlendStateCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        logicOpEnable: false as _,
        logicOp: br::vk::VK_LOGIC_OP_CLEAR,
        attachmentCount: color_blend_states.len() as _,
        pAttachments: color_blend_states.as_ptr(),
        blendConstants: [0.0; 4]
    };
    let pipeline_cinfo = br::vk::VkGraphicsPipelineCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        layout: ps_layout.as_ptr(),
        renderPass: rp.as_ptr(),
        subpass: 0,
        stageCount: shader_stage_cinfos.len() as _,
        pStages: shader_stage_cinfos.as_ptr(),
        pVertexInputState: &vertex_input_state_cinfo,
        pInputAssemblyState: &input_assembly_state_cinfo,
        pViewportState: &viewport_state_cinfo,
        pRasterizationState: &rasterization_state_cinfo,
        pMultisampleState: &multisample_state_cinfo,
        pColorBlendState: &blend_state_cinfo,
        .. unsafe { std::mem::MaybeUninit::zeroed().assume_init() }
    };
    let mut ps = vec![br::vk::VK_NULL_HANDLE as _];
    let r = unsafe { br::vk::vkCreateGraphicsPipelines(vk_device.as_ptr(), br::vk::VK_NULL_HANDLE as _, 1, &pipeline_cinfo, std::ptr::null(), ps.as_mut_ptr()) };
    vk_to_result(r).expect("vkCreateGraphicsPipelines failed");
    let ps = UniqueObject(ps[0], |p| unsafe { br::vk::vkDestroyPipeline(vk_device.as_ptr(), p, std::ptr::null()); });

    // Create Shared Object from Swapchain Backbuffers
    let vk_get_memory_win32_handle_properties_khr: br::vk::PFN_vkGetMemoryWin32HandlePropertiesKHR = unsafe {
        std::mem::transmute(
            br::vk::vkGetDeviceProcAddr(vk_device.as_ptr(), b"vkGetMemoryWin32HandlePropertiesKHR\0".as_ptr() as _)
                .expect("vkGetMemoryWin32HandlePropertiesKHR not found?")
        )
    };
    let vk_backbuffers = (0..2).map(|n| {
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

    let fence_cinfo = br::vk::VkFenceCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0
    };
    let mut fence = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateFence(vk_device.as_ptr(), &fence_cinfo, std::ptr::null(), &mut fence) };
    vk_to_result(r).expect("vkCreateFence failed");
    let fence = UniqueObject(fence, |p| unsafe { br::vk::vkDestroyFence(vk_device.as_ptr(), p, std::ptr::null()); });

    let transfer_cp_cinfo = br::vk::VkCommandPoolCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: br::vk::VK_COMMAND_POOL_CREATE_TRANSIENT_BIT,
        queueFamilyIndex: queue_family_index as _
    };
    let mut cp = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateCommandPool(vk_device.as_ptr(), &transfer_cp_cinfo, std::ptr::null(), &mut cp) };
    vk_to_result(r).expect("vkCreateCommandPool failed");
    let cp = UniqueObject(cp, |p| unsafe { br::vk::vkDestroyCommandPool(vk_device.as_ptr(), p, std::ptr::null()); });
    let transfer_cmd_ainfo = br::vk::VkCommandBufferAllocateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        pNext: std::ptr::null(),
        commandPool: cp.as_ptr(),
        level: br::vk::VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        commandBufferCount: 1
    };
    let mut transfer_cmd = vec![br::vk::VK_NULL_HANDLE as _];
    let r = unsafe { br::vk::vkAllocateCommandBuffers(vk_device.as_ptr(), &transfer_cmd_ainfo, transfer_cmd.as_mut_ptr()) };
    vk_to_result(r).expect("vkAllocateCommandBUffers failed");
    let cmd_begin_info = br::vk::VkCommandBufferBeginInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        pNext: std::ptr::null(),
        flags: br::vk::VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
        pInheritanceInfo: std::ptr::null()
    };
    let r = unsafe { br::vk::vkBeginCommandBuffer(transfer_cmd[0], &cmd_begin_info) };
    vk_to_result(r).expect("vkBeginCommandBuffer failed");
    let in_buffer_barriers = &[
        br::vk::VkBufferMemoryBarrier {
            sType: br::vk::VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,
            pNext: std::ptr::null(),
            srcAccessMask: br::vk::VK_ACCESS_HOST_WRITE_BIT,
            dstAccessMask: br::vk::VK_ACCESS_TRANSFER_READ_BIT,
            srcQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
            dstQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
            buffer: stg_buffer.as_ptr(),
            offset: 0,
            size: buf_size as _
        },
        br::vk::VkBufferMemoryBarrier {
            sType: br::vk::VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,
            pNext: std::ptr::null(),
            srcAccessMask: 0,
            dstAccessMask: br::vk::VK_ACCESS_TRANSFER_WRITE_BIT,
            srcQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
            dstQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
            buffer: buffer.as_ptr(),
            offset: 0,
            size: buf_size as _
        }
    ];
    let out_buffer_barriers = &[
        br::vk::VkBufferMemoryBarrier {
            srcAccessMask: in_buffer_barriers[0].dstAccessMask,
            dstAccessMask: br::vk::VK_ACCESS_HOST_WRITE_BIT,
            .. in_buffer_barriers[0]
        },
        br::vk::VkBufferMemoryBarrier {
            srcAccessMask: in_buffer_barriers[1].dstAccessMask,
            dstAccessMask: br::vk::VK_ACCESS_VERTEX_ATTRIBUTE_READ_BIT | br::vk::VK_ACCESS_UNIFORM_READ_BIT,
            .. in_buffer_barriers[1]
        }
    ];
    let out_image_barriers = vk_backbuffers.iter().map(|(_, _, o, _, _)| br::vk::VkImageMemoryBarrier {
        sType: br::vk::VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        pNext: std::ptr::null(),
        srcAccessMask: 0,
        dstAccessMask: br::vk::VK_ACCESS_MEMORY_READ_BIT,
        srcQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
        dstQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
        oldLayout: br::vk::VK_IMAGE_LAYOUT_PREINITIALIZED,
        newLayout: br::vk::VK_IMAGE_LAYOUT_GENERAL,
        image: o.as_ptr(),
        subresourceRange: br::vk::VkImageSubresourceRange {
            aspectMask: br::vk::VK_IMAGE_ASPECT_COLOR_BIT,
            baseMipLevel: 0,
            levelCount: 1,
            baseArrayLayer: 0,
            layerCount: 1
        }
    }).collect::<Vec<_>>();
    let copy_buffer_ranges = &[
        br::vk::VkBufferCopy { srcOffset: 0, dstOffset: 0, size: buf_size as _ }
    ];
    let r = unsafe {
        br::vk::vkCmdPipelineBarrier(
            transfer_cmd[0], br::vk::VK_PIPELINE_STAGE_HOST_BIT, br::vk::VK_PIPELINE_STAGE_TRANSFER_BIT, 0,
            0, std::ptr::null(), in_buffer_barriers.len() as _, in_buffer_barriers.as_ptr(), 0, std::ptr::null()
        );
        br::vk::vkCmdCopyBuffer(transfer_cmd[0], stg_buffer.as_ptr(), buffer.as_ptr(), copy_buffer_ranges.len() as _, copy_buffer_ranges.as_ptr());
        br::vk::vkCmdPipelineBarrier(
            transfer_cmd[0], br::vk::VK_PIPELINE_STAGE_TRANSFER_BIT,
            br::vk::VK_PIPELINE_STAGE_HOST_BIT | br::vk::VK_PIPELINE_STAGE_VERTEX_INPUT_BIT | br::vk::VK_PIPELINE_STAGE_VERTEX_SHADER_BIT, 0,
            0, std::ptr::null(), out_buffer_barriers.len() as _, out_buffer_barriers.as_ptr(), out_image_barriers.len() as _, out_image_barriers.as_ptr()
        );
        br::vk::vkEndCommandBuffer(transfer_cmd[0])
    };
    vk_to_result(r).expect("Recording TransferCommands failed");
    let transfer_submit_infos = &[
        br::vk::VkSubmitInfo {
            sType: br::vk::VK_STRUCTURE_TYPE_SUBMIT_INFO,
            pNext: std::ptr::null(),
            commandBufferCount: transfer_cmd.len() as _,
            pCommandBuffers: transfer_cmd.as_ptr(),
            waitSemaphoreCount: 0,
            pWaitSemaphores: std::ptr::null(),
            pWaitDstStageMask: std::ptr::null(),
            signalSemaphoreCount: 0,
            pSignalSemaphores: std::ptr::null()
        }
    ];
    let r = unsafe { br::vk::vkQueueSubmit(vk_queue, transfer_submit_infos.len() as _, transfer_submit_infos.as_ptr(), fence.as_ptr()) };
    vk_to_result(r).expect("vkQueueSubmit failed");
    let r = unsafe { br::vk::vkWaitForFences(vk_device.as_ptr(), 1, &fence.as_ptr(), false as _, std::u64::MAX) };
    vk_to_result(r).expect("vkWaitForFences failed");
    unsafe { br::vk::vkFreeCommandBuffers(vk_device.as_ptr(), cp.as_ptr(), transfer_cmd.len() as _, transfer_cmd.as_ptr()) };
    drop(transfer_cmd);

    let cp_cinfo = br::vk::VkCommandPoolCreateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        queueFamilyIndex: queue_family_index as _
    };
    let mut cp = br::vk::VK_NULL_HANDLE as _;
    let r = unsafe { br::vk::vkCreateCommandPool(vk_device.as_ptr(), &cp_cinfo, std::ptr::null(), &mut cp) };
    vk_to_result(r).expect("vkCreateCommandPool failed");
    let cp = UniqueObject(cp, |p| unsafe { br::vk::vkDestroyCommandPool(vk_device.as_ptr(), p, std::ptr::null()); });
    let cmd_ainfo = br::vk::VkCommandBufferAllocateInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        pNext: std::ptr::null(),
        commandPool: cp.as_ptr(),
        level: br::vk::VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        commandBufferCount: vk_backbuffers.len() as _
    };
    let mut command_buffers = vec![br::vk::VK_NULL_HANDLE as _; vk_backbuffers.len()];
    let r = unsafe { br::vk::vkAllocateCommandBuffers(vk_device.as_ptr(), &cmd_ainfo, command_buffers.as_mut_ptr()) };
    vk_to_result(r).expect("vkAllocateCommandBuffers failed");
    let cmd_begin_info = br::vk::VkCommandBufferBeginInfo {
        sType: br::vk::VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        pNext: std::ptr::null(),
        flags: 0,
        pInheritanceInfo: std::ptr::null()
    };
    let update_buffer_barrier_in = &[
        br::vk::VkBufferMemoryBarrier {
            sType: br::vk::VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,
            pNext: std::ptr::null(),
            srcAccessMask: br::vk::VK_ACCESS_HOST_WRITE_BIT,
            dstAccessMask: br::vk::VK_ACCESS_TRANSFER_READ_BIT,
            srcQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
            dstQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
            buffer: stg_buffer.as_ptr(),
            offset: 0,
            size: std::mem::size_of::<TimerUniform>() as _
        },
        br::vk::VkBufferMemoryBarrier {
            sType: br::vk::VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,
            pNext: std::ptr::null(),
            srcAccessMask: br::vk::VK_ACCESS_UNIFORM_READ_BIT,
            dstAccessMask: br::vk::VK_ACCESS_TRANSFER_WRITE_BIT,
            srcQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
            dstQueueFamilyIndex: br::vk::VK_QUEUE_FAMILY_IGNORED,
            buffer: buffer.as_ptr(), 
            offset: 0,
            size: std::mem::size_of::<TimerUniform>() as _
        }
    ];
    let update_buffer_barrier_out = &[
        br::vk::VkBufferMemoryBarrier {
            srcAccessMask: update_buffer_barrier_in[0].dstAccessMask,
            dstAccessMask: update_buffer_barrier_in[0].srcAccessMask,
            .. update_buffer_barrier_in[0]
        },
        br::vk::VkBufferMemoryBarrier {
            srcAccessMask: update_buffer_barrier_in[1].dstAccessMask,
            dstAccessMask: update_buffer_barrier_in[1].srcAccessMask,
            .. update_buffer_barrier_in[1]
        }
    ];
    let update_buffer_range = &[
        br::vk::VkBufferCopy { srcOffset: 0, dstOffset: 0, size: std::mem::size_of::<TimerUniform>() as _ }
    ];
    let clear_values = &[
        br::vk::VkClearValue { color: br::vk::VkClearColorValue { float32: [0.0; 4] } }
    ];
    let render_vbufs = &[buffer.as_ptr()];
    let render_vbuf_offsets = &[buf_offset_vertices as _];
    for (&cmd, fb) in command_buffers.iter().zip(vk_backbuffers.iter().map(|(_, _, _, _, fb)| fb)) {
        let rp_begin_info = br::vk::VkRenderPassBeginInfo {
            sType: br::vk::VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
            pNext: std::ptr::null(),
            renderPass: rp.as_ptr(),
            framebuffer: fb.as_ptr(),
            renderArea: br::vk::VkRect2D {
                offset: br::vk::VkOffset2D { x: 0, y: 0 },
                extent: br::vk::VkExtent2D { width: 640, height: 480 }
            },
            clearValueCount: clear_values.len() as _,
            pClearValues: clear_values.as_ptr()
        };

        let r = unsafe { br::vk::vkBeginCommandBuffer(cmd, &cmd_begin_info) };
        vk_to_result(r).expect("vkBeginCommandBuffer failed");
        let r = unsafe {
            // update
            br::vk::vkCmdPipelineBarrier(
                cmd,
                br::vk::VK_PIPELINE_STAGE_HOST_BIT | br::vk::VK_PIPELINE_STAGE_VERTEX_SHADER_BIT,
                br::vk::VK_PIPELINE_STAGE_TRANSFER_BIT, 0,
                0, std::ptr::null(), update_buffer_barrier_in.len() as _, update_buffer_barrier_in.as_ptr(), 0, std::ptr::null()
            );
            br::vk::vkCmdCopyBuffer(cmd, stg_buffer.as_ptr(), buffer.as_ptr(), update_buffer_range.len() as _, update_buffer_range.as_ptr());
            br::vk::vkCmdPipelineBarrier(
                cmd, br::vk::VK_PIPELINE_STAGE_TRANSFER_BIT,
                br::vk::VK_PIPELINE_STAGE_HOST_BIT | br::vk::VK_PIPELINE_STAGE_VERTEX_SHADER_BIT, 0,
                0, std::ptr::null(), update_buffer_barrier_out.len() as _, update_buffer_barrier_out.as_ptr(), 0, std::ptr::null()
            );
    
            // render
            br::vk::vkCmdBeginRenderPass(cmd, &rp_begin_info, br::vk::VK_SUBPASS_CONTENTS_INLINE);
            br::vk::vkCmdBindPipeline(cmd, br::vk::VK_PIPELINE_BIND_POINT_GRAPHICS, ps.as_ptr());
            br::vk::vkCmdBindDescriptorSets(cmd, br::vk::VK_PIPELINE_BIND_POINT_GRAPHICS, ps_layout.as_ptr(), 0, 1, sets.as_ptr(), 0, std::ptr::null());
            br::vk::vkCmdBindVertexBuffers(cmd, 0, 1, render_vbufs.as_ptr(), render_vbuf_offsets.as_ptr());
            br::vk::vkCmdDraw(cmd, 3, 1, 0, 0);
            br::vk::vkCmdEndRenderPass(cmd);

            br::vk::vkEndCommandBuffer(cmd)
        };
        vk_to_result(r).expect("Recording RenderCommands failed");
    }

    let fence_event = unsafe { winapi::um::synchapi::CreateEventA(std::ptr::null_mut(), false as _, true as _, b"FenceEvent\0".as_ptr() as _) };

    let mut msg = unsafe { std::mem::MaybeUninit::uninit().assume_init() };
    let mut timer = std::time::Instant::now();
    let mut fence_value = 1;
    'brk: loop {
        while unsafe { PeekMessageA(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 } {
            if msg.message == WM_QUIT { break 'brk; }

            unsafe {
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }
        }

        let r = unsafe { br::vk::vkGetFenceStatus(vk_device.as_ptr(), fence.as_ptr()) };
        if r == br::vk::VK_SUCCESS {
            // update/render
            let dtms = timer.elapsed().as_micros() as f32 / 1_000_000.0;
            timer = std::time::Instant::now();

            unsafe {
                let handles = &[sc_waitable, fence_event];
                winapi::um::synchapi::WaitForMultipleObjectsEx(handles.len() as _, handles.as_ptr(), true as _, winapi::um::winbase::INFINITE, false as _)
            };
            let hr = unsafe { sc.Present(0, 0) };
            hr_to_ioresult(hr).expect("SwapChain Present failed");
            let hr = unsafe { cq.Signal(fence12.as_ptr(), fence_value) };
            hr_to_ioresult(hr).expect("Fence signaling failed");
            let hr = unsafe { fence12.SetEventOnCompletion(fence_value, fence_event) };
            hr_to_ioresult(hr).expect("Fence Event Setting failed");
            fence_value += 1;

            let mut p = std::ptr::null_mut();
            let r = unsafe { br::vk::vkMapMemory(vk_device.as_ptr(), stg_buffer_mem.as_ptr(), 0, std::mem::size_of::<TimerUniform>() as _, 0, &mut p) };
            vk_to_result(r).expect("vkMapMemory update failed");
            unsafe { (*(p as *mut TimerUniform)).time += dtms; }
            if needs_stg_memory_cache_flush {
                let ranges = &[br::vk::VkMappedMemoryRange {
                    sType: br::vk::VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE,
                    pNext: std::ptr::null(),
                    memory: stg_buffer_mem.as_ptr(),
                    offset: 0,
                    size: std::mem::size_of::<TimerUniform>() as _
                }];
                let r = unsafe { br::vk::vkFlushMappedMemoryRanges(vk_device.as_ptr(), ranges.len() as _, ranges.as_ptr()) };
                vk_to_result(r).expect("vkFlushMappedMemoryRanges update failed");
            }
            unsafe { br::vk::vkUnmapMemory(vk_device.as_ptr(), stg_buffer_mem.as_ptr()) };

            let r = unsafe { br::vk::vkResetFences(vk_device.as_ptr(), 1, &fence.as_ptr()) };
            vk_to_result(r).expect("vkResetFences failed");
            let next = unsafe { sc.GetCurrentBackBufferIndex() };
            let submit_infos = &[
                br::vk::VkSubmitInfo {
                    sType: br::vk::VK_STRUCTURE_TYPE_SUBMIT_INFO,
                    pNext: std::ptr::null(),
                    commandBufferCount: 1,
                    pCommandBuffers: unsafe { command_buffers.as_ptr().add(next as usize) },
                    .. unsafe { std::mem::MaybeUninit::zeroed().assume_init() }
                }
            ];
            let r = unsafe { br::vk::vkQueueSubmit(vk_queue, submit_infos.len() as _, submit_infos.as_ptr(), fence.as_ptr()) };
            vk_to_result(r).expect("vkQueueSubmit loop failed");
        }
    }

    let r = unsafe { br::vk::vkDeviceWaitIdle(vk_device.as_ptr()) };
    vk_to_result(r).expect("vkDeviceWaitIdle failed");
    unsafe { winapi::um::synchapi::WaitForSingleObject(fence_event, winapi::um::winbase::INFINITE) };
    unsafe { br::vk::vkFreeCommandBuffers(vk_device.as_ptr(), cp.as_ptr(), command_buffers.len() as _, command_buffers.as_ptr()) };
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
