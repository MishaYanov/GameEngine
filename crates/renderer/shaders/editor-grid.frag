#version 450
layout(location=0) in vec2 ndc;
layout(location=0) out vec4 outColor;
layout(push_constant, std430) uniform GridData {
    mat4 inverseViewProjection;
    vec4 originSpacing;
    vec4 tangentOpacity;
    vec4 normalFade;
    vec4 viewport;
} grid;

float safeW(float value) { return abs(value) < 1e-8 ? (value < 0.0 ? -1e-8 : 1e-8) : value; }
float coverage(vec2 uv, vec2 derivative) {
    vec2 distance = abs(fract(uv - 0.5) - 0.5) / max(derivative, vec2(1e-6));
    return 1.0 - clamp(min(distance.x, distance.y), 0.0, 1.0);
}
vec3 axisColor(vec3 axis) {
    return abs(axis.x) > 0.5 ? vec3(0.74,0.30,0.27)
         : abs(axis.y) > 0.5 ? vec3(0.30,0.66,0.42) : vec3(0.29,0.48,0.77);
}
void main() {
    vec4 nearH=grid.inverseViewProjection*vec4(ndc,0.0,1.0);
    vec4 farH=grid.inverseViewProjection*vec4(ndc,1.0,1.0);
    vec3 nearPoint=nearH.xyz/safeW(nearH.w);
    vec3 farPoint=farH.xyz/safeW(farH.w);
    vec3 ray=farPoint-nearPoint;
    vec3 normal=grid.normalFade.xyz;
    float denominator=dot(normal,ray);
    float t=dot(normal,grid.originSpacing.xyz-nearPoint)/safeW(denominator);
    vec3 world=nearPoint+t*ray;
    vec3 tangent=grid.tangentOpacity.xyz;
    vec3 bitangent=cross(normal,tangent);
    vec3 relative=world-grid.originSpacing.xyz;
    vec2 uv=vec2(dot(relative,tangent),dot(relative,bitangent))/grid.originSpacing.w;
    // Derivatives precede fragment-varying rejection, including the horizon.
    vec2 derivative=max(fwidth(uv),vec2(1e-6));
    float minor=coverage(uv,derivative);
    float major=coverage(uv/10.0,derivative/10.0);
    float fineFade=1.0-smoothstep(0.12,0.55,max(derivative.x,derivative.y));
    float majorFade=1.0-smoothstep(0.12,0.55,max(derivative.x,derivative.y)/10.0);
    float alpha=max(minor*0.40*fineFade,major*0.80*majorFade);
    vec3 color=mix(vec3(0.43,0.47,0.53),vec3(0.62,0.66,0.72),major);
    vec2 axis=1.0-clamp(abs(uv)/derivative,0.0,1.0);
    if(axis.y>0.0)color=mix(color,axisColor(tangent),axis.y);
    if(axis.x>0.0)color=mix(color,axisColor(bitangent),axis.x);
    alpha=max(alpha,max(axis.x,axis.y));
    float distance=length(world-nearPoint);
    float distanceFade=1.0-smoothstep(grid.normalFade.w*0.6,grid.normalFade.w,distance);
    float angleFade=smoothstep(0.008,0.08,abs(denominator)/max(length(ray),1e-6));
    float depthDenominator=farH.w+t*(nearH.w-farH.w);
    float depth=t*nearH.w/safeW(depthDenominator);
    if(abs(denominator)<1e-6||t<0.0||t>1.0||depth<0.0||depth>1.0||isnan(depth)||isinf(depth))discard;
    alpha*=grid.tangentOpacity.w*distanceFade*angleFade;
    if(alpha<0.001)discard;
    gl_FragDepth=max(0.0,depth-0.000002);
    outColor=vec4(color,alpha);
}
