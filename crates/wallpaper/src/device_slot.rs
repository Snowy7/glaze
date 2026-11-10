/// A device can be absent only during recovery. All old references must be released before
/// D3D12CreateDevice can reopen the adapter's removed device; callers must not draw meanwhile.
pub struct DeviceSlot<T>(Option<T>);

impl<T> From<T> for DeviceSlot<T> {
    fn from(device: T) -> Self {
        Self(Some(device))
    }
}

impl<T> DeviceSlot<T> {
    pub fn get(&self) -> Option<&T> {
        self.0.as_ref()
    }
    pub fn clear(&mut self) {
        self.0 = None;
    }
    pub fn replace(&mut self, device: T) {
        self.0 = Some(device);
    }
}

impl<T> std::ops::Deref for DeviceSlot<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0.as_ref().expect("drawing during device recovery")
    }
}
