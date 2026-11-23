#version 450
layout(location = 0) in vec2 screen_ndc;
layout(location = 0) out vec4 color;

layout(push_constant) uniform Sky {
    vec4 ray_x;
    vec4 ray_y;
    vec4 forward;
    vec4 zenith;
    vec4 horizon;
} sky;

void main() {
    vec3 ray = normalize(sky.forward.xyz + screen_ndc.x * sky.ray_x.xyz + screen_ndc.y * sky.ray_y.xyz);
    float height = smoothstep(0.0, 1.0, ray.y);
    color = vec4(mix(sky.horizon.rgb, sky.zenith.rgb, height), 1.0);
}
