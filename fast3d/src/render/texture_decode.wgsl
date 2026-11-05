struct DecodeParams {
    extent: vec4<u32>,
    recipe: vec4<u32>,
    addressing: vec4<u32>,
    input: vec4<u32>,
}

@group(0) @binding(0) var<storage, read> encoded: array<u32>;
@group(0) @binding(1) var<uniform> params: DecodeParams;
@group(0) @binding(2) var output: texture_storage_2d<rgba8unorm, write>;

fn framebuffer_pointer(address: u32) -> u32 {
    if params.input.y == 0u { return 0u; }
    return encoded[params.input.y + address];
}

fn framebuffer_color(pointer: u32) -> vec4<f32> {
    return unpack4x8unorm(encoded[(pointer - 1u) >> 1u]);
}

fn framebuffer_ia(color: vec4<f32>) -> vec4<f32> {
    // rt64 preserves precision with R10G10B10A2 interpreted as I16A16.
    let rgba = vec4<u32>(round(clamp(color * 1023.0, vec4<f32>(0.0), vec4<f32>(1023.0))));
    let packed = (rgba.r << 22u) | (rgba.g << 12u) | (rgba.b << 2u) | select(0u, 3u, rgba.a > 0u);
    let intensity = f32(packed >> 16u) / 65535.0;
    return vec4<f32>(intensity, intensity, intensity, f32(packed & 65535u) / 65535.0);
}

fn byte_at(address: u32) -> u32 {
    let pointer = framebuffer_pointer(address);
    if pointer != 0u {
        let color = vec4<u32>(round(framebuffer_color(pointer) * 255.0));
        let packed = ((color.r >> 3u) << 11u) | ((color.g >> 3u) << 6u) | ((color.b >> 3u) << 1u) | ((color.a >> 2u) & 1u);
        return (packed >> (8u * (1u - ((pointer - 1u) & 1u)))) & 255u;
    }
    // Upload bytes stay in N64 order: byte 0 occupies bits 0..7 of each little-endian word.
    return (encoded[address >> 2u] >> ((address & 3u) * 8u)) & 255u;
}

fn rgba16(value: u32) -> vec4<u32> {
    let rgb = vec3<u32>((value >> 11u) & 31u, (value >> 6u) & 31u, (value >> 1u) & 31u);
    return vec4<u32>((rgb << vec3<u32>(3u)) | (rgb >> vec3<u32>(2u)), (value & 1u) * 255u);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= params.extent.xy) {
        return;
    }
    let representation = params.recipe.x;
    let format = params.recipe.y;
    let size = params.recipe.z;
    var parity = id.x & 1u;
    var odd_swap = (id.y & 1u) * 4u;
    var relative = id.y * params.addressing.x + ((id.x << min(size, 2u)) >> 1u);
    if representation == 1u {
        relative = id.x;
        parity = id.y & 1u;
        odd_swap = (id.y >> 1u) * 4u;
    }
    let mask = select(4095u, 2047u, format == 2u || size == 3u);
    let address0 = (params.recipe.w + (relative ^ odd_swap)) & mask;
    let address1 = (params.recipe.w + ((relative + 1u) ^ odd_swap)) & mask;
    let pointer = framebuffer_pointer(address0);
    if size == 2u && pointer != 0u && ((pointer - 1u) & 1u) == 0u && framebuffer_pointer(address1) == pointer + 1u {
        let color = framebuffer_color(pointer);
        if format == 0u {
            textureStore(output, vec2<i32>(id.xy), color);
            return;
        }
        if format == 3u {
            textureStore(output, vec2<i32>(id.xy), framebuffer_ia(color));
            return;
        }
    }
    let first = byte_at(address0);
    var second = 0u;
    if size >= 2u {
        second = byte_at(address1);
    }
    let nibble = (first >> ((1u - parity) * 4u)) & 15u;
    var color = vec4<u32>(0u);
    switch format * 4u + size {
        case 2u: {
            color = rgba16((first << 8u) | second);
        }
        case 3u: {
            let blue = byte_at(address0 | 2048u);
            let alpha = byte_at(address1 | 2048u);
            color = vec4<u32>(first, second, blue, alpha);
        }
        case 8u, 9u: {
            var index = first;
            if size == 0u {
                index = params.addressing.y * 16u + nibble;
            }
            let palette = (2048u + index * 8u) & 4095u;
            let hi = byte_at(palette);
            let lo = byte_at(palette + 1u);
            if params.addressing.z == 2u {
                color = rgba16((hi << 8u) | lo);
            } else if params.addressing.z == 3u {
                color = vec4<u32>(hi, hi, hi, lo);
            }
        }
        case 12u: {
            let intensity = nibble & 14u;
            let expanded = (intensity << 4u) | (intensity << 1u) | (intensity >> 2u);
            color = vec4<u32>(expanded, expanded, expanded, (nibble & 1u) * 255u);
        }
        case 13u: {
            let intensity = (first >> 4u) * 17u;
            color = vec4<u32>(intensity, intensity, intensity, (first & 15u) * 17u);
        }
        case 14u: {
            color = vec4<u32>(first, first, first, second);
        }
        case 16u: { color = vec4<u32>(nibble * 17u); }
        case 17u: { color = vec4<u32>(first); }
        default: {}
    }
    textureStore(output, vec2<i32>(id.xy), vec4<f32>(color) / 255.0);
}
