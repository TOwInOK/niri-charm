precision highp float;
uniform float niri_alpha;
uniform float niri_scale;
uniform vec2 niri_size;
varying vec2 niri_v_coords;
uniform sampler2D image;
uniform float source_size;
uniform float zoom;

uniform float grid_from;
uniform float grid_enabled;
uniform vec4 cross_color;
uniform vec4 border_color;
uniform float border_width;
uniform vec4 grid_color;
void main() {
    vec2 p = (niri_v_coords - 0.5) * niri_size / niri_scale;
    vec2 a = abs(p);
    float radius = min(niri_size.x, niri_size.y) * 0.5 / niri_scale;
    float edge = radius - length(p);
    if (edge <= 0.0) discard;
    vec2 cell = p / zoom;
    vec2 uv = vec2(0.5) + cell / source_size;
    vec4 color = texture2D(image, uv);
    // Missing source pixels at output edges remain neutral, not edge-stretched.
    color = vec4(color.rgb + vec3(0.12) * (1.0 - color.a), 1.0);
    // Mark the cursor pixel row and column, preserving the center pixel color.
    bool column = a.x < zoom * 0.5;
    bool row = a.y < zoom * 0.5;
    if (column != row) {
        color.rgb = mix(color.rgb, cross_color.rgb, cross_color.a);
    }
    if (grid_enabled > 0.5 && zoom >= grid_from) {
        vec2 boundary = fract(cell + 0.5) * zoom;
        float distance_to_grid = min(boundary.x, boundary.y);
        float line = 1.0 - step(1.0 / niri_scale, distance_to_grid);
        color.rgb = mix(color.rgb, grid_color.rgb, grid_color.a * line);
    }
    // Highlight the exact source pixel under the cursor without filling it.
    float center_edge = max(a.x, a.y) - zoom * 0.5;
    if (abs(center_edge) < 1.0 / niri_scale) {
        color.rgb = vec3(center_edge < 0.0 ? 1.0 : 0.0);
    }
    float border = clamp((border_width - edge) * niri_scale + 0.5, 0.0, 1.0);
    if (border_width > 0.0) {
        color.rgb = mix(color.rgb, border_color.rgb, border_color.a * border);
    }
    gl_FragColor = color * clamp(edge * niri_scale, 0.0, 1.0) * niri_alpha;
}
