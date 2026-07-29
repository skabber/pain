struct Globals {
    screen_size: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas_tex: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
// Glyphs that carry their own colors (emoji) — see `atlas::COLOR_ATLAS_SIZE`
// for why these are a separate texture rather than one widened atlas.
@group(0) @binding(3) var color_atlas_tex: texture_2d<f32>;

struct VertexInput {
    @location(0) corner: vec2<f32>,
};

struct InstanceInput {
    @location(1) pos: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) uv_origin: vec2<f32>,
    @location(4) uv_size: vec2<f32>,
    @location(5) color: vec4<f32>,
    @location(6) colored: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) colored: f32,
};

@vertex
fn vs_main(v: VertexInput, inst: InstanceInput) -> VertexOutput {
    let pixel_pos = inst.pos + v.corner * inst.size;
    let ndc_x = (pixel_pos.x / globals.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pixel_pos.y / globals.screen_size.y) * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = inst.uv_origin + v.corner * inst.uv_size;
    out.color = inst.color;
    out.colored = inst.colored;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // A glyph the font draws in its own colors (an emoji). Its texels are
    // already premultiplied on upload (see `glyph::GlyphPixels::Color`), so
    // they composite directly — scaled only by the instance's alpha, which
    // carries window transparency, never by its RGB, which would tint an
    // emoji with the surrounding text color and destroy the very thing that
    // makes it a color glyph.
    if in.colored > 0.5 {
        return textureSample(color_atlas_tex, atlas_sampler, in.uv) * in.color.a;
    }

    let coverage = textureSample(atlas_tex, atlas_sampler, in.uv).r;
    let alpha = in.color.a * coverage;
    // Premultiplied output: RGB scaled by the *effective* alpha (instance
    // alpha times glyph-edge coverage), not just the instance's own alpha —
    // required by the pipeline's `PREMULTIPLIED_ALPHA_BLENDING` blend state
    // (see `GridRenderer::new`), and in turn by `DXGI_ALPHA_MODE_PREMULTIPLIED`,
    // the only alpha mode Windows' DirectComposition swapchains accept
    // (confirmed via the D3D12 debug layer after `STRAIGHT` was rejected
    // outright: "Composition SwapChains do not support the
    // DXGI_ALPHA_MODE_STRAIGHT AlphaMode"). Solid rects (background,
    // cursor, dividers, selection) always sample full coverage from the
    // atlas's reserved opaque texel, so this is a no-op for them beyond
    // their own alpha; only anti-aliased glyph edges actually need the
    // per-pixel coverage folded in here rather than on the CPU side.
    return vec4<f32>(in.color.rgb * alpha, alpha);
}
