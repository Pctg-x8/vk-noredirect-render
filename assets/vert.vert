#version 450

layout(location = 0) in vec4 pos;
layout(location = 1) in vec4 color;

out gl_PerVertex { out vec4 gl_Position; };
layout(location = 0) out vec4 o_color;

layout(set = 0, binding = 0) uniform timer {
    float time;
};

void main() {
    const vec2 cs = vec2(cos(time * 2.0), sin(time * 2.0));
    const mat2 matrix = mat2(vec2(cs.x, -cs.y), vec2(cs.y, cs.x));
    gl_Position = vec4(matrix * pos.xy, pos.zw);
    o_color = color;
}
