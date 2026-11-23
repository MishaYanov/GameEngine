#version 450
layout(location = 0) in vec2 screen_ndc;
layout(location = 0) out vec4 color;

layout(set = 0, binding = 0) uniform sampler2D panorama;
layout(push_constant) uniform Sky {
    vec4 ray_x;
    vec4 ray_y;
    vec4 forward;
    vec4 zenith;
    vec4 horizon;
} sky;

void main() {
    const float pi = 3.14159265359;
    vec3 ray = normalize(sky.forward.xyz + screen_ndc.x * sky.ray_x.xyz + screen_ndc.y * sky.ray_y.xyz);
    float longitude = dot(ray.xz, ray.xz) > 1e-12 ? atan(ray.x, -ray.z) / (2.0 * pi) : 0.0;
    float u = fract(0.5 + longitude - sky.forward.w);
    float v = acos(clamp(ray.y, -1.0, 1.0)) / pi;
    float half_texel = 0.5 / float(textureSize(panorama, 0).y);
    vec3 linear_sample = texture(panorama, vec2(u, clamp(v, half_texel, 1.0 - half_texel))).rgb;
    vec3 tint = mix(sky.horizon.rgb / vec3(0.30, 0.46, 0.65),
        sky.zenith.rgb / vec3(0.035, 0.075, 0.18), smoothstep(0.0, 1.0, ray.y));
    color = vec4(linear_sample * min(tint, vec3(3.0)) * sky.zenith.w, 1.0);
}
