use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct BlockFlags: u16 {
        const NONE          = 0;
        const SOLID         = 1 << 0;
        const OPAQUE        = 1 << 1;
        const TRANSPARENT   = 1 << 2;
        const EMISSIVE      = 1 << 3;
        const NO_CULL       = 1 << 4;
        const REPLACEABLE   = 1 << 5;
        const LIQUID        = 1 << 6;
        const CLIMBABLE     = 1 << 7;
    }
}
