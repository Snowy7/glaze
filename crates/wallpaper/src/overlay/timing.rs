//! GPU time of the overlays' wgpu passes, from timestamp queries, when the device has them
//! (`Features::TIMESTAMP_QUERY`; the engine's device doesn't ask for it, the spike's does). Each
//! read waits for the GPU, so it is for measuring, not for the engine's loop.

/// Passes timed per submission: begin and end of each.
const PASSES: u32 = 8;

pub struct Timer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// ns per tick.
    period: f32,
    used: u32,
}

impl Timer {
    /// `None` without timestamp queries.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let bytes = u64::from(PASSES * 2) * 8;
        let buffer = |usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("glass overlay timestamps"),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(Self {
            set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("glass overlay timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: PASSES * 2,
            }),
            resolve: buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            readback: buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
            period: queue.get_timestamp_period(),
            used: 0,
        })
    }

    /// The writes for the next pass of this submission, while there is room.
    pub fn render_pass(&mut self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let i = self.next()?;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(i),
            end_of_pass_write_index: Some(i + 1),
        })
    }

    pub fn compute_pass(&mut self) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
        let i = self.next()?;
        Some(wgpu::ComputePassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(i),
            end_of_pass_write_index: Some(i + 1),
        })
    }

    fn next(&mut self) -> Option<u32> {
        (self.used < PASSES * 2).then(|| {
            self.used += 2;
            self.used - 2
        })
    }

    /// Records resolving this submission's timestamps (last, in its encoder).
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.used == 0 {
            return;
        }
        encoder.resolve_query_set(&self.set, 0..self.used, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve,
            0,
            &self.readback,
            0,
            u64::from(self.used) * 8,
        );
    }

    /// After the submission: the summed GPU ms of its timed passes (waits for the GPU).
    pub fn read(&mut self, device: &wgpu::Device) -> f64 {
        let used = std::mem::take(&mut self.used);
        if used == 0 {
            return 0.0;
        }
        let slice = self.readback.slice(..u64::from(used) * 8);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let ticks: Vec<u64> = slice
            .get_mapped_range()
            .map(|bytes| bytemuck::pod_collect_to_vec(&bytes[..]))
            .unwrap_or_default();
        self.readback.unmap();
        let sum: u64 = ticks
            .chunks(2)
            .map(|pair| pair[1].saturating_sub(pair[0]))
            .sum();
        sum as f64 * f64::from(self.period) / 1e6
    }
}
