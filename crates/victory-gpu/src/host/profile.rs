//! What the host GPU is, read once when the device is made ([`read`]), so
//! that code which treats one vendor's cards or one capability differently
//! asks here instead of querying DXGI and D3D12 itself.
//!
//! This fork is tuned on AMD RDNA. A path for another vendor (NVIDIA first)
//! is chosen here and nowhere else: by [`Profile::nvidia`] for a default the
//! measurements on that vendor's cards ask for ([`crate::switch::by_host`]),
//! or by a capability field for what the card can or cannot do. A host whose
//! vendor is not known (no adapter named, a profile not read yet) is taken
//! for AMD's: every default stays what it was before.

//!
//! (Offline translation, `victory_shader::emit::host::Host::translation_caps`,
//! keeps its own AMD check: any edit under `victory-shader/src` changes the
//! translator's fingerprint, `build.rs`, and every cached shader is
//! translated again.)

use std::sync::OnceLock;

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, IDXGIAdapter1, IDXGIFactory4};

/// The maker of a host GPU, by the PCI vendor ID its adapter reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Vendor {
    Amd,
    Nvidia,
    Intel,
    /// The software adapter (WARP) and the basic display driver.
    Microsoft,
    Other(u32),
}

impl Vendor {
    pub(crate) const AMD_ID: u32 = 0x1002;
    pub(crate) const NVIDIA_ID: u32 = 0x10de;
    pub(crate) const INTEL_ID: u32 = 0x8086;
    pub(crate) const MICROSOFT_ID: u32 = 0x1414;

    pub(crate) fn from_id(id: u32) -> Self {
        match id {
            Self::AMD_ID => Self::Amd,
            Self::NVIDIA_ID => Self::Nvidia,
            Self::INTEL_ID => Self::Intel,
            Self::MICROSOFT_ID => Self::Microsoft,
            other => Self::Other(other),
        }
    }

    pub(crate) fn id(self) -> u32 {
        match self {
            Self::Amd => Self::AMD_ID,
            Self::Nvidia => Self::NVIDIA_ID,
            Self::Intel => Self::INTEL_ID,
            Self::Microsoft => Self::MICROSOFT_ID,
            Self::Other(id) => id,
        }
    }
}

/// The vendor of the adapter `device` was made on (`None`: DXGI does not
/// name the adapter).
pub(crate) fn vendor_of(device: &ID3D12Device) -> Option<Vendor> {
    // SAFETY: plain adapter queries for a live device.
    unsafe {
        CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0))
            .and_then(|f| f.EnumAdapterByLuid::<IDXGIAdapter1>(device.GetAdapterLuid()))
            .and_then(|a| a.GetDesc1())
            .ok()
            .map(|d| Vendor::from_id(d.VendorId))
    }
}

/// The device's `(WaveLaneCountMin, WaveLaneCountMax)` (`None`: the query
/// failed).
fn wave_lanes(device: &ID3D12Device) -> Option<(u32, u32)> {
    ask::<D3D12_FEATURE_DATA_D3D12_OPTIONS1>(device, D3D12_FEATURE_D3D12_OPTIONS1)
        .map(|o| (o.WaveLaneCountMin, o.WaveLaneCountMax))
}

/// Whether a host is AMD RDNA: an AMD card that runs waves of 32 and of 64
/// (the vendor's older cards run 64 alone).
fn is_rdna(vendor: Option<Vendor>, (min, max): (u32, u32)) -> bool {
    vendor == Some(Vendor::Amd) && min == 32 && max == 64
}

/// Feature `feature`'s data, if the runtime knows the feature.
fn ask<T: Default>(device: &ID3D12Device, feature: D3D12_FEATURE) -> Option<T> {
    let mut data = T::default();
    // SAFETY: a feature structure of the requested type and size; an older
    // runtime that does not know the feature fails the query.
    unsafe { device.CheckFeatureSupport(feature, (&raw mut data).cast(), size_of::<T>() as u32) }
        .ok()
        .map(|()| data)
}

/// The host GPU's vendor and the capabilities the layer chooses paths by.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Profile {
    /// `None`: DXGI did not name the device's adapter.
    pub(crate) vendor: Option<Vendor>,
    /// `(WaveLaneCountMin, WaveLaneCountMax)`; `None`: not reported.
    pub(crate) wave_lanes: Option<(u32, u32)>,
    /// `D3D12_RESOURCE_BINDING_TIER` (0: not reported).
    pub(crate) binding_tier: i32,
    /// The most views a shader-visible view heap may hold, as the runtime
    /// reports it (`D3D12_OPTIONS19`; `None`: a runtime too old to say).
    /// `descriptor::host::probe_view_limit` still decides the heap's size by
    /// making one: this is what the card says, for the log and for paths
    /// that must know before a heap is made.
    pub(crate) max_views: Option<u32>,
    /// `D3D12_TILED_RESOURCES_TIER` (0: none, or not reported).
    pub(crate) tiled_tier: i32,
    /// Buffers in the card's memory that the CPU maps
    /// (`D3D12_HEAP_TYPE_GPU_UPLOAD`: a resizable BAR, and a system and
    /// driver that offer it).
    pub(crate) gpu_upload_heap: bool,
    /// Anisotropic filtering with point mip filtering (`D3D12_OPTIONS19`).
    pub(crate) aniso_point_mip: bool,
}

impl Profile {
    /// Whether the host is an AMD card.
    pub(crate) fn amd(&self) -> bool {
        self.vendor == Some(Vendor::Amd)
    }

    /// Whether the host is an NVIDIA card.
    pub(crate) fn nvidia(&self) -> bool {
        self.vendor == Some(Vendor::Nvidia)
    }

    /// Whether the host is AMD RDNA (waves of 32 and of 64).
    pub(crate) fn rdna(&self) -> bool {
        self.wave_lanes.is_some_and(|lanes| is_rdna(self.vendor, lanes))
    }
}

static PROFILE: OnceLock<Profile> = OnceLock::new();

/// The host GPU's profile, once the host device is made (`None` before, and
/// without a host device).
pub(crate) fn profile() -> Option<&'static Profile> {
    PROFILE.get()
}

/// The profile, if `device` is the host device (which is not made here).
pub(crate) fn of_host_device(device: &ID3D12Device) -> Option<&'static Profile> {
    let host = super::HOST.get()?.as_ref()?;
    (host.device == *device).then_some(profile()?)
}

/// Whether the host GPU is an NVIDIA card. Makes the host device if it is
/// not made yet, so a default chosen by it is never read before the vendor
/// is known; not to be called while the device is being made.
pub(crate) fn nvidia_host() -> bool {
    if PROFILE.get().is_none() {
        let _ = super::host();
    }
    profile().is_some_and(Profile::nvidia)
}

/// Reads `device`'s profile, keeps it for [`profile`] (the first one read:
/// the host device's) and logs it once.
pub(crate) fn read(device: &ID3D12Device) -> &'static Profile {
    PROFILE.get_or_init(|| {
        let profile = query(device);
        tracing::info!(
            vendor = ?profile.vendor,
            vendor_id = profile.vendor.map(|v| format!("{:#06x}", v.id())),
            wave_lanes = ?profile.wave_lanes,
            binding_tier = profile.binding_tier,
            max_views = profile.max_views,
            tiled_tier = profile.tiled_tier,
            gpu_upload_heap = profile.gpu_upload_heap,
            aniso_point_mip = profile.aniso_point_mip,
            "host GPU profile"
        );
        profile
    })
}

fn query(device: &ID3D12Device) -> Profile {
    let options = ask::<D3D12_FEATURE_DATA_D3D12_OPTIONS>(device, D3D12_FEATURE_D3D12_OPTIONS);
    let options16 = ask::<D3D12_FEATURE_DATA_D3D12_OPTIONS16>(device, D3D12_FEATURE_D3D12_OPTIONS16);
    let options19 = ask::<D3D12_FEATURE_DATA_D3D12_OPTIONS19>(device, D3D12_FEATURE_D3D12_OPTIONS19);
    Profile {
        vendor: vendor_of(device),
        wave_lanes: wave_lanes(device),
        binding_tier: options.map_or(0, |o| o.ResourceBindingTier.0),
        max_views: options19.map(|o| o.MaxViewDescriptorHeapSize).filter(|&n| n > 0),
        tiled_tier: options.map_or(0, |o| o.TiledResourcesTier.0),
        gpu_upload_heap: options16.is_some_and(|o| o.GPUUploadHeapSupported.as_bool()),
        aniso_point_mip: options19.is_some_and(|o| o.AnisoFilterWithPointMipSupported.as_bool()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Profile, Vendor, is_rdna};

    #[test]
    fn vendors_are_told_apart_by_their_pci_ids() {
        assert_eq!(Vendor::from_id(0x1002), Vendor::Amd);
        assert_eq!(Vendor::from_id(0x10de), Vendor::Nvidia);
        assert_eq!(Vendor::from_id(0x8086), Vendor::Intel);
        assert_eq!(Vendor::from_id(0x1414), Vendor::Microsoft);
        assert_eq!(Vendor::from_id(0x5143), Vendor::Other(0x5143));
        for id in [0x1002, 0x10de, 0x8086, 0x1414, 0x5143] {
            assert_eq!(Vendor::from_id(id).id(), id);
        }
    }

    #[test]
    fn rdna_is_an_amd_card_with_both_wave_sizes() {
        assert!(is_rdna(Some(Vendor::Amd), (32, 64)));
        // GCN (Polaris, Vega): waves of 64 alone.
        assert!(!is_rdna(Some(Vendor::Amd), (64, 64)));
        assert!(!is_rdna(Some(Vendor::Nvidia), (32, 32)));
        assert!(!is_rdna(Some(Vendor::Intel), (32, 64)));
        assert!(!is_rdna(None, (32, 64)));
    }

    fn of(vendor: Option<Vendor>, wave_lanes: Option<(u32, u32)>) -> Profile {
        Profile {
            vendor,
            wave_lanes,
            binding_tier: 3,
            max_views: None,
            tiled_tier: 3,
            gpu_upload_heap: false,
            aniso_point_mip: false,
        }
    }

    #[test]
    fn an_rx_7900_is_amd_rdna_and_not_nvidia() {
        let p = of(Some(Vendor::Amd), Some((32, 64)));
        assert!(p.amd() && p.rdna() && !p.nvidia());
    }

    #[test]
    fn an_rtx_card_is_nvidia_and_not_rdna() {
        let p = of(Some(Vendor::Nvidia), Some((32, 32)));
        assert!(p.nvidia() && !p.amd() && !p.rdna());
    }

    #[test]
    fn an_older_amd_card_or_unknown_lanes_is_not_rdna() {
        assert!(!of(Some(Vendor::Amd), Some((64, 64))).rdna());
        assert!(!of(Some(Vendor::Amd), None).rdna());
    }

    #[test]
    fn an_unnamed_adapter_is_neither() {
        let p = of(None, Some((32, 64)));
        assert!(!p.amd() && !p.nvidia() && !p.rdna());
    }
}
