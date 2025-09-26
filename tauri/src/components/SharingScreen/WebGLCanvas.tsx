import React from "react";

type Props = {
  className?: string;
  style?: React.CSSProperties;
  width?: number;
  height?: number;
};

export const WebGLCanvas = React.forwardRef<HTMLCanvasElement, Props>(function WebGLCanvas(
  { className, style, width, height }: Props,
  forwardedRef,
) {
  const canvasRef = React.useRef<HTMLCanvasElement>(null);

  React.useImperativeHandle(forwardedRef, () => canvasRef.current as HTMLCanvasElement, []);

  React.useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    // Initialize WebGL context
    const gl = canvas.getContext("webgl2") || canvas.getContext("webgl");
    if (!gl) {
      console.error("WebGL not supported");
      return;
    }

    // Store the WebGL context for later use
    (canvas as any).__webglContext = gl;

    // Cleanup function to prevent memory leaks
    return () => {
      const renderer = (canvas as any).__webglRenderer as WebGLRenderer;
      if (renderer) {
        cleanupWebGLRenderer(renderer);
        (canvas as any).__webglRenderer = null;
      }
      (canvas as any).__webglContext = null;
    };
  }, []);

  return <canvas ref={canvasRef} className={className} style={style} width={width} height={height} />;
});

// Vertex shader source
const vertexShaderSource = `
  attribute vec2 a_position;
  attribute vec2 a_texCoord;
  varying vec2 v_texCoord;

  void main() {
    gl_Position = vec4(a_position, 0.0, 1.0);
    v_texCoord = a_texCoord;
  }
`;

// Fragment shader source for I420 to RGB conversion
const fragmentShaderSource = `
  precision mediump float;

  uniform sampler2D u_textureY;
  uniform sampler2D u_textureU;
  uniform sampler2D u_textureV;
  uniform bool u_fullRange;
  varying vec2 v_texCoord;

  void main() {
    float y = texture2D(u_textureY, v_texCoord).r;
    float u = texture2D(u_textureU, v_texCoord).r - 0.5;
    float v = texture2D(u_textureV, v_texCoord).r - 0.5;

    if (!u_fullRange) {
      // Limited range YUV (16-235 for Y, 16-240 for UV)
      // Scale Y from [16/255, 235/255] to [0, 1]
      y = (y - 16.0/255.0) * 255.0/219.0;
      // Scale UV from [-112/255, 112/255] to [-0.5, 0.5]
      u = u * 255.0/224.0;
      v = v * 255.0/224.0;
    }

    // BT.709 matrix conversion
    float r = y + 1.5748 * v;
    float g = y - 0.1873 * u - 0.4681 * v;
    float b = y + 1.8556 * u;

    // Ensure full range output [0, 1]
    r = clamp(r, 0.0, 1.0);
    g = clamp(g, 0.0, 1.0);
    b = clamp(b, 0.0, 1.0);

    gl_FragColor = vec4(r, g, b, 1.0);
  }
`;

function createShader(gl: WebGLRenderingContext, type: number, source: string): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) return null;

  gl.shaderSource(shader, source);
  gl.compileShader(shader);

  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    console.error('Shader compilation error:', gl.getShaderInfoLog(shader));
    gl.deleteShader(shader);
    return null;
  }

  return shader;
}

function createProgram(gl: WebGLRenderingContext, vertexShader: WebGLShader, fragmentShader: WebGLShader): WebGLProgram | null {
  const program = gl.createProgram();
  if (!program) return null;

  gl.attachShader(program, vertexShader);
  gl.attachShader(program, fragmentShader);
  gl.linkProgram(program);

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.error('Program linking error:', gl.getProgramInfoLog(program));
    gl.deleteProgram(program);
    return null;
  }

  return program;
}

interface WebGLRenderer {
  gl: WebGLRenderingContext;
  program: WebGLProgram;
  positionBuffer: WebGLBuffer;
  texCoordBuffer: WebGLBuffer;
  textureY: WebGLTexture;
  textureU: WebGLTexture;
  textureV: WebGLTexture;
  locations: {
    position: number;
    texCoord: number;
    textureY: WebGLUniformLocation;
    textureU: WebGLUniformLocation;
    textureV: WebGLUniformLocation;
    fullRange: WebGLUniformLocation;
  };
  // Cache for texture parameters to avoid redundant state changes
  lastTextureParams?: {
    yWidth: number;
    yHeight: number;
    uvWidth: number;
    uvHeight: number;
  };
}

function cleanupWebGLRenderer(renderer: WebGLRenderer) {
  const { gl } = renderer;

  // Delete textures
  gl.deleteTexture(renderer.textureY);
  gl.deleteTexture(renderer.textureU);
  gl.deleteTexture(renderer.textureV);

  // Delete buffers
  gl.deleteBuffer(renderer.positionBuffer);
  gl.deleteBuffer(renderer.texCoordBuffer);

  // Delete program (this also detaches shaders)
  gl.deleteProgram(renderer.program);
}

function initializeWebGLRenderer(gl: WebGLRenderingContext): WebGLRenderer | null {
  const vertexShader = createShader(gl, gl.VERTEX_SHADER, vertexShaderSource);
  const fragmentShader = createShader(gl, gl.FRAGMENT_SHADER, fragmentShaderSource);

  if (!vertexShader || !fragmentShader) {
    // Clean up any created shaders
    if (vertexShader) gl.deleteShader(vertexShader);
    if (fragmentShader) gl.deleteShader(fragmentShader);
    return null;
  }

  const program = createProgram(gl, vertexShader, fragmentShader);

  // Clean up shaders after linking (they're now part of the program)
  gl.deleteShader(vertexShader);
  gl.deleteShader(fragmentShader);

  if (!program) return null;

  // Create buffers for quad vertices
  const positionBuffer = gl.createBuffer();
  if (!positionBuffer) return null;

  gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
  const positions = new Float32Array([
    -1, -1,
     1, -1,
    -1,  1,
     1,  1,
  ]);
  gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);

  // Create texture coordinate buffer
  const texCoordBuffer = gl.createBuffer();
  if (!texCoordBuffer) return null;

  gl.bindBuffer(gl.ARRAY_BUFFER, texCoordBuffer);
  const texCoords = new Float32Array([
    0, 1,
    1, 1,
    0, 0,
    1, 0,
  ]);
  gl.bufferData(gl.ARRAY_BUFFER, texCoords, gl.STATIC_DRAW);

  // Create textures
  const textureY = gl.createTexture();
  const textureU = gl.createTexture();
  const textureV = gl.createTexture();

  if (!textureY || !textureU || !textureV) return null;

  // Get attribute and uniform locations
  const positionLocation = gl.getAttribLocation(program, 'a_position');
  const texCoordLocation = gl.getAttribLocation(program, 'a_texCoord');
  const textureYLocation = gl.getUniformLocation(program, 'u_textureY');
  const textureULocation = gl.getUniformLocation(program, 'u_textureU');
  const textureVLocation = gl.getUniformLocation(program, 'u_textureV');
  const fullRangeLocation = gl.getUniformLocation(program, 'u_fullRange');

  if (textureYLocation === null || textureULocation === null || textureVLocation === null || fullRangeLocation === null) {
    return null;
  }

  return {
    gl,
    program,
    positionBuffer,
    texCoordBuffer,
    textureY,
    textureU,
    textureV,
    locations: {
      position: positionLocation,
      texCoord: texCoordLocation,
      textureY: textureYLocation,
      textureU: textureULocation,
      textureV: textureVLocation,
      fullRange: fullRangeLocation,
    },
  };
}

function calculateDisplaySize(width: number, height: number) {
  // For now, render at source resolution; hook for future scaling or DPR handling.
  return { displayWidth: width, displayHeight: height, scaleX: 1 };
}

export function drawI420FrameToCanvasWebGL(
  canvas: HTMLCanvasElement,
  yData: Uint8Array,
  uData: Uint8Array,
  vData: Uint8Array,
  width: number,
  height: number,
  timestamp: number,
  onMetrics?: (captureToBeforeDrawMs: number, captureToAfterDrawMs: number) => void,
  fullRange: boolean = false,
) {
  const gl = (canvas as any).__webglContext as WebGLRenderingContext;
  if (!gl) {
    console.error("WebGL context not found");
    return;
  }

  // Initialize renderer if not already done
  let renderer = (canvas as any).__webglRenderer as WebGLRenderer;
  if (!renderer) {
    const newRenderer = initializeWebGLRenderer(gl);
    if (!newRenderer) {
      console.error("Failed to initialize WebGL renderer");
      return;
    }
    renderer = newRenderer;
    (canvas as any).__webglRenderer = renderer;
  }

  const beforeDrawMs = Date.now();

  const { displayWidth, displayHeight } = calculateDisplaySize(width, height);
  if (canvas.width !== displayWidth || canvas.height !== displayHeight) {
    canvas.width = displayWidth;
    canvas.height = displayHeight;
    gl.viewport(0, 0, displayWidth, displayHeight);
  }

  const uvWidth = width >> 1;
  const uvHeight = height >> 1;

  // Check if texture dimensions changed to avoid redundant texture parameter calls
  const dimensionsChanged = !renderer.lastTextureParams ||
    renderer.lastTextureParams.yWidth !== width ||
    renderer.lastTextureParams.yHeight !== height ||
    renderer.lastTextureParams.uvWidth !== uvWidth ||
    renderer.lastTextureParams.uvHeight !== uvHeight;

  // Upload Y plane texture
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, renderer.textureY);
  if (dimensionsChanged) {
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    // Allocate texture storage
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.LUMINANCE, width, height, 0, gl.LUMINANCE, gl.UNSIGNED_BYTE, null);
  }
  // Update texture data (more efficient than texImage2D when dimensions haven't changed)
  gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, width, height, gl.LUMINANCE, gl.UNSIGNED_BYTE, yData);

  // Upload U plane texture
  gl.activeTexture(gl.TEXTURE1);
  gl.bindTexture(gl.TEXTURE_2D, renderer.textureU);
  if (dimensionsChanged) {
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    // Allocate texture storage
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.LUMINANCE, uvWidth, uvHeight, 0, gl.LUMINANCE, gl.UNSIGNED_BYTE, null);
  }
  // Update texture data
  gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, uvWidth, uvHeight, gl.LUMINANCE, gl.UNSIGNED_BYTE, uData);

  // Upload V plane texture
  gl.activeTexture(gl.TEXTURE2);
  gl.bindTexture(gl.TEXTURE_2D, renderer.textureV);
  if (dimensionsChanged) {
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    // Allocate texture storage
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.LUMINANCE, uvWidth, uvHeight, 0, gl.LUMINANCE, gl.UNSIGNED_BYTE, null);

    // Cache the texture parameters
    renderer.lastTextureParams = { yWidth: width, yHeight: height, uvWidth, uvHeight };
  }
  // Update texture data
  gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, uvWidth, uvHeight, gl.LUMINANCE, gl.UNSIGNED_BYTE, vData);

  // Use the shader program
  gl.useProgram(renderer.program);

  // Set up uniforms (these are lightweight and don't need caching)
  gl.uniform1i(renderer.locations.textureY, 0);
  gl.uniform1i(renderer.locations.textureU, 1);
  gl.uniform1i(renderer.locations.textureV, 2);
  gl.uniform1i(renderer.locations.fullRange, fullRange ? 1 : 0);

  // Set up attributes
  gl.bindBuffer(gl.ARRAY_BUFFER, renderer.positionBuffer);
  gl.enableVertexAttribArray(renderer.locations.position);
  gl.vertexAttribPointer(renderer.locations.position, 2, gl.FLOAT, false, 0, 0);

  gl.bindBuffer(gl.ARRAY_BUFFER, renderer.texCoordBuffer);
  gl.enableVertexAttribArray(renderer.locations.texCoord);
  gl.vertexAttribPointer(renderer.locations.texCoord, 2, gl.FLOAT, false, 0, 0);

  // Draw the quad
  gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

  const afterDrawMs = Date.now();
  if (onMetrics) onMetrics(beforeDrawMs, afterDrawMs);
}

// Export cleanup function for external use
export function cleanupWebGLCanvas(canvas: HTMLCanvasElement) {
  const renderer = (canvas as any).__webglRenderer as WebGLRenderer;
  if (renderer) {
    cleanupWebGLRenderer(renderer);
    (canvas as any).__webglRenderer = null;
  }
  (canvas as any).__webglContext = null;
}
