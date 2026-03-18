use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct BlockFlags: u16 {
        const NONE          = 0;
        const SOLID         = 1 << 0;
        const OPAQUE        = 1 << 1;
        const TRANSPARENT   = 1 << 2;
        const NO_CULL       = 1 << 3;
        const REPLACEABLE   = 1 << 4;
        const LIQUID        = 1 << 5;
        const RAYCAST_THROUGH = 1 << 6;
    }
}
