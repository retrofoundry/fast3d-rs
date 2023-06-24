// For now these are defined same as WGPU

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum CompareFunction {
    /// Function never passes
    Never = 1,
    /// Function passes if new value less than existing value
    Less = 2,
    /// Function passes if new value is equal to existing value. When using
    /// this compare function, make sure to mark your Vertex Shader's `@builtin(position)`
    /// output as `@invariant` to prevent artifacting.
    Equal = 3,
    /// Function passes if new value is less than or equal to existing value
    LessEqual = 4,
    /// Function passes if new value is greater than existing value
    Greater = 5,
    /// Function passes if new value is not equal to existing value. When using
    /// this compare function, make sure to mark your Vertex Shader's `@builtin(position)`
    /// output as `@invariant` to prevent artifacting.
    NotEqual = 6,
    /// Function passes if new value is greater than or equal to existing value
    GreaterEqual = 7,
    /// Function always passes
    Always = 8,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// Front face
    Front = 0,
    /// Back face
    Back = 1,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum BlendFactor {
    /// 0.0
    Zero = 0,
    /// 1.0
    One = 1,
    /// S.component
    Src = 2,
    /// 1.0 - S.component
    OneMinusSrc = 3,
    /// S.alpha
    SrcAlpha = 4,
    /// 1.0 - S.alpha
    OneMinusSrcAlpha = 5,
    /// D.component
    Dst = 6,
    /// 1.0 - D.component
    OneMinusDst = 7,
    /// D.alpha
    DstAlpha = 8,
    /// 1.0 - D.alpha
    OneMinusDstAlpha = 9,
    /// min(S.alpha, 1.0 - D.alpha)
    SrcAlphaSaturated = 10,
    /// Constant
    Constant = 11,
    /// 1.0 - Constant
    OneMinusConstant = 12,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
pub enum BlendOperation {
    /// Src + Dst
    #[default]
    Add = 0,
    /// Src - Dst
    Subtract = 1,
    /// Dst - Src
    ReverseSubtract = 2,
    /// min(Src, Dst)
    Min = 3,
    /// max(Src, Dst)
    Max = 4,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlendComponent {
    /// Multiplier for the source, which is produced by the fragment shader.
    pub src_factor: BlendFactor,
    /// Multiplier for the destination, which is stored in the target.
    pub dst_factor: BlendFactor,
    /// The binary operation applied to the source and destination,
    /// multiplied by their respective factors.
    pub operation: BlendOperation,
}

impl BlendComponent {
    /// Default blending state that replaces destination with the source.
    pub const REPLACE: Self = Self {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::Zero,
        operation: BlendOperation::Add,
    };

    /// Blend state of (1 * src) + ((1 - src_alpha) * dst)
    pub const OVER: Self = Self {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    };

    /// Returns true if the state relies on the constant color, which is
    /// set independently on a render command encoder.
    pub fn uses_constant(&self) -> bool {
        match (self.src_factor, self.dst_factor) {
            (BlendFactor::Constant, _)
            | (BlendFactor::OneMinusConstant, _)
            | (_, BlendFactor::Constant)
            | (_, BlendFactor::OneMinusConstant) => true,
            (_, _) => false,
        }
    }
}

impl Default for BlendComponent {
    fn default() -> Self {
        Self::REPLACE
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlendState {
    /// Color equation.
    pub color: BlendComponent,
    /// Alpha equation.
    pub alpha: BlendComponent,
}

impl BlendState {
    /// Blend mode that does no color blending, just overwrites the output with the contents of the shader.
    pub const REPLACE: Self = Self {
        color: BlendComponent::REPLACE,
        alpha: BlendComponent::REPLACE,
    };

    /// Blend mode that does standard alpha blending with non-premultiplied alpha.
    pub const ALPHA_BLENDING: Self = Self {
        color: BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent::OVER,
    };

    /// Blend mode that does standard alpha blending with premultiplied alpha.
    pub const PREMULTIPLIED_ALPHA_BLENDING: Self = Self {
        color: BlendComponent::OVER,
        alpha: BlendComponent::OVER,
    };
}
