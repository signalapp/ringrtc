//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import { Call } from './Service';

// Match a React.RefObject without relying on React.
interface Ref<T> {
  readonly current: T | null;
}

// The way a CanvasVideoRender gets VideoFrames
export interface VideoFrameSource {
  // Fills in the given buffer and returns the width x height
  // or returns undefined if nothing was filled in because no
  // video frame was available.
  receiveVideoFrame(buffer: ArrayBuffer): [number, number] | undefined;
}

// The way a GumVideoCapturer sends frames
interface VideoFrameSender {
  sendVideoFrame(width: number, height: number, rgbaBuffer: ArrayBuffer): void;
}

export class GumVideoCaptureOptions {
  maxWidth: number = 640;
  maxHeight: number = 480;
  maxFramerate: number = 30;
  preferredDeviceId?: string;
  screenShareSourceId?: string;
}
export class GumVideoCapturer {
  private defaultCaptureOptions: GumVideoCaptureOptions;
  private localPreview?: Ref<HTMLVideoElement>;
  private captureOptions?: GumVideoCaptureOptions;
  private getUserMediaPromise?: Promise<MediaStream>;
  private sender?: VideoFrameSender;
  private mediaStream?: MediaStream;
  private canvas?: OffscreenCanvas;
  private canvasContext?: OffscreenCanvasRenderingContext2D;
  private intervalId?: any;
  private preferredDeviceId?: string;
  private capturingStartTime: number | undefined;
  // Set this if you want fake video
  public fakeVideoName: string | undefined;

  constructor(defaultCaptureOptions: GumVideoCaptureOptions) {
    this.defaultCaptureOptions = defaultCaptureOptions;
  }

  capturing() {
    return this.captureOptions != undefined;
  }

  setLocalPreview(localPreview: Ref<HTMLVideoElement> | undefined) {
    this.localPreview = localPreview;
  }

  enableCapture(): void {
    // tslint:disable no-floating-promises
    this.startCapturing(this.defaultCaptureOptions);
  }

  enableCaptureAndSend(sender: VideoFrameSender, options?: GumVideoCaptureOptions): void {
    // tslint:disable no-floating-promises
    this.startCapturing(options ?? this.defaultCaptureOptions);
    this.startSending(sender);
  }

  disable(): void {
    this.stopCapturing();
    this.stopSending();
  }

  async setPreferredDevice(deviceId: string): Promise<void> {
    this.preferredDeviceId = deviceId;

    if (this.captureOptions) {
      const captureOptions = this.captureOptions;
      const sender = this.sender;

      this.disable();
      this.startCapturing(captureOptions);
      if (sender) {
        this.startSending(sender);
      }
    }
  }

  async enumerateDevices(): Promise<MediaDeviceInfo[]> {
    const devices = await window.navigator.mediaDevices.enumerateDevices();
    const cameras = devices.filter(d => d.kind == "videoinput");
    return cameras;
  }

  // This helps prevent concurrent calls to `getUserMedia`.
  private getUserMedia(options: GumVideoCaptureOptions): Promise<MediaStream> {
    if (!this.getUserMediaPromise) {
      let contraints: any = {
        audio: false,
        video: {
          deviceId: options.preferredDeviceId ?? this.preferredDeviceId,
          width: {
            max: options.maxWidth,
          },
          height: {
            max: options.maxHeight,
          },
          frameRate: {
            max: options.maxFramerate,
          },
        },
      };
      if (options.screenShareSourceId != undefined) {
        contraints.video = {
          mandatory: {
            chromeMediaSource: 'desktop',
            chromeMediaSourceId: options.screenShareSourceId,
            maxWidth: options.maxWidth,
            maxHeight: options.maxHeight,
            maxFrameRate: options.maxFramerate,
          }
        };
      }
      // TODO: Figure out a better way to make typescript accept "mandatory".
      this.getUserMediaPromise = window.navigator.mediaDevices.getUserMedia(contraints).then(mediaStream => {
          delete this.getUserMediaPromise;
          return mediaStream;
        });
    }
    return this.getUserMediaPromise;
  }

  private async startCapturing(options: GumVideoCaptureOptions): Promise<void> {
    if (this.capturing()) {
      return;
    }
    this.captureOptions = options;
    this.capturingStartTime = Date.now();
    try {
      const mediaStream = await this.getUserMedia(options);
      // We could have been disabled between when we requested the stream
      // and when we got it.
      if (this.captureOptions != options) {
        for (const track of mediaStream.getVideoTracks()) {
          // Make the light turn off faster
          track.stop();
        }
        return;
      }

      if (this.localPreview && !!this.localPreview.current && !!mediaStream) {
        this.setLocalPreviewSourceObject(mediaStream);
      }
      this.mediaStream = mediaStream;
    } catch {
      // We couldn't open the camera.  Oh well.
    }
  }

  private stopCapturing(): void {
    if (!this.capturing()) {
      return;
    }
    this.captureOptions = undefined;
    if (!!this.mediaStream) {
      for (const track of this.mediaStream.getVideoTracks()) {
        // Make the light turn off faster
        track.stop();
      }
      this.mediaStream = undefined;
    }
    if (this.localPreview && !!this.localPreview.current) {
      this.localPreview.current.srcObject = null;
    }
  }

  private startSending(sender: VideoFrameSender): void {
    if (this.sender === sender) {
      return;
    }
    if (!!this.sender) {
      // If we're replacing an existing sender, make sure we stop the
      // current setInterval loop before starting another one.
      this.stopSending();
    }
    this.sender = sender;
    this.canvas = new OffscreenCanvas(this.captureOptions!.maxWidth, this.captureOptions!.maxHeight);
    this.canvasContext = this.canvas.getContext('2d') || undefined;
    const interval = 1000 / this.captureOptions!.maxFramerate;
    this.intervalId = setInterval(
      this.captureAndSendOneVideoFrame.bind(this),
      interval
    );
  }

  private stopSending(): void {
    this.sender = undefined;
    this.canvas = undefined;
    this.canvasContext = undefined;
    if (!!this.intervalId) {
      clearInterval(this.intervalId);
    }
  }

  private setLocalPreviewSourceObject(mediaStream: MediaStream): void {
    if (!this.localPreview) {
      return;
    }
    const localPreview = this.localPreview.current;
    if (!localPreview) {
      return;
    }

    localPreview.srcObject = mediaStream;
    // I don't know why this is necessary
    if (localPreview.width === 0) {
      localPreview.width = this.captureOptions!.maxWidth;
    }
    if (localPreview.height === 0) {
      localPreview.height = this.captureOptions!.maxHeight;
    }
  }

  private captureAndSendOneVideoFrame(): void {
    if (!this.canvas || !this.canvasContext || !this.sender) {
      return;
    }

    if ((this.fakeVideoName != undefined) && (this.capturingStartTime != undefined)) {
      let width = 640;
      let height = 480;
      let duration = Date.now() - this.capturingStartTime;
      this.drawFakeVideo(this.canvasContext, width, height, this.fakeVideoName, duration);
      const image = this.canvasContext.getImageData(0, 0, width, height);
      this.sender.sendVideoFrame(image.width, image.height, image.data.buffer);
      return;
    }

    if (this.localPreview && this.localPreview.current) {
      if (!this.localPreview.current.srcObject && !!this.mediaStream) {
        this.setLocalPreviewSourceObject(this.mediaStream);
      }
      const width = this.localPreview.current.videoWidth;
      const height = this.localPreview.current.videoHeight;
      if (width === 0 || height === 0) {
        return;
      }

      this.canvasContext.drawImage(
        this.localPreview.current,
        0,
        0,
        width,
        height
      );
      const image = this.canvasContext.getImageData(0, 0, width, height);
      this.sender.sendVideoFrame(image.width, image.height, image.data.buffer);
    }
  }

  private drawFakeVideo(context: OffscreenCanvasRenderingContext2D, width: number, height: number, name: string, time: number): void {
    function fill(style: string, draw: () => void) {
      context.fillStyle = style;
      context.beginPath();
      draw();
      context.fill();
    }

    function stroke(style: string, draw: () => void) {
      context.strokeStyle = style;
      context.beginPath();
      draw();
      context.stroke();
    }

    function arc(x: number, y: number, radius: number, start: number, end: number) {
      const twoPi = 2 * Math.PI;
      context.arc(x, y, radius, start * twoPi, end * twoPi);
    }

    function circle(x: number, y: number, radius: number) {
      arc(x, y, radius, 0, 1);
    }

    function fillFace(x: number, y: number, faceRadius: number) {
      const eyeRadius = faceRadius/5;
      const eyeOffsetX = faceRadius/2;
      const eyeOffsetY = -faceRadius/4;
      const smileRadius = faceRadius/2;
      const smileOffsetY = -eyeOffsetY;
      fill("yellow", () => circle(x, y, faceRadius));
      fill("black", () => circle(x - eyeOffsetX, y + eyeOffsetY, eyeRadius));
      fill("black", () => circle(x + eyeOffsetX, y + eyeOffsetY, eyeRadius));
      stroke("black", () => arc(x, y + smileOffsetY, smileRadius, 0, 0.5));
    }

    function fillText(x: number, y: number, fillStyle: string, fontSize: number, fontName: string, align: CanvasTextAlign, text: string) {
      context.font = `${fontSize}px ${fontName}`;
      context.textAlign = align;
      context.fillStyle = fillStyle;
      context.fillText(text, x, y);
    }

    function fillLabeledFace(x: number, y: number, faceRadius: number, label: string) {
      const labelSize = faceRadius*.3;
      const labelOffsetY = faceRadius*1.5;

      fillFace(x, y, faceRadius);
      fillText(x, y + labelOffsetY, "black", labelSize, "monospace", "center", label);
    }

    context.fillStyle = 'white';
    context.fillRect(0, 0, width, height);
    fillLabeledFace(width/2, height/2, height/3, `${name} ${time}`);
  }
}

// We add 10% in each dimension to allow for things that are slightly wider or taller than 1080p.
const MAX_VIDEO_CAPTURE_MULTIPLIER = 1.0;
export const MAX_VIDEO_CAPTURE_WIDTH = 1920 * MAX_VIDEO_CAPTURE_MULTIPLIER;
export const MAX_VIDEO_CAPTURE_HEIGHT = 1080 * MAX_VIDEO_CAPTURE_MULTIPLIER;
export const MAX_VIDEO_CAPTURE_AREA = MAX_VIDEO_CAPTURE_WIDTH * MAX_VIDEO_CAPTURE_HEIGHT;
export const MAX_VIDEO_CAPTURE_BUFFER_SIZE = MAX_VIDEO_CAPTURE_AREA * 4;

export class CanvasVideoRenderer {
  private canvas?: Ref<HTMLCanvasElement>;
  private buffer: ArrayBuffer;
  private source?: VideoFrameSource;
  private rafId?: any;

  constructor() {
    this.buffer = new ArrayBuffer(MAX_VIDEO_CAPTURE_BUFFER_SIZE);
  }

  setCanvas(canvas: Ref<HTMLCanvasElement> | undefined) {
    this.canvas = canvas;
  }

  enable(source: VideoFrameSource): void {
    if (this.source === source) {
      return;
    }
    if (!!this.source) {
      // If we're replacing an existing source, make sure we stop the
      // current rAF loop before starting another one.
      if (this.rafId) {
        window.cancelAnimationFrame(this.rafId);
      }
    }
    this.source = source;
    this.requestAnimationFrameCallback();
  }

  disable() {
    this.renderBlack();
    this.source = undefined;
    if (this.rafId) {
      window.cancelAnimationFrame(this.rafId);
    }
  }

  private requestAnimationFrameCallback() {
    this.renderVideoFrame();
    this.rafId = window.requestAnimationFrame(this.requestAnimationFrameCallback.bind(this));
  }

  private renderBlack() {
    if (!this.canvas) {
      return;
    }
    const canvas = this.canvas.current;
    if (!canvas) {
      return;
    }
    const context = canvas.getContext('2d');
    if (!context) {
      return;
    }
    context.fillStyle = 'black';
    context.fillRect(0, 0, canvas.width, canvas.height);
  }

  private renderVideoFrame() {
    if (!this.source || !this.canvas) {
      return;
    }
    const canvas = this.canvas.current;
    if (!canvas) {
      return;
    }
    const context = canvas.getContext('2d');
    if (!context) {
      return;
    }

    const frame = this.source.receiveVideoFrame(this.buffer);
    if (!frame) {
      return;
    }
    const [ width, height ] = frame;

    if (canvas.clientWidth <= 0 || width <= 0 ||
        canvas.clientHeight <= 0 || height <= 0) {
      return;
    }

    const frameAspectRatio = width / height;
    const canvasAspectRatio = canvas.clientWidth / canvas.clientHeight;

    let dx = 0;
    let dy = 0;
    if (frameAspectRatio > canvasAspectRatio) {
      // Frame wider than view: We need bars at the top and bottom
      canvas.width = width;
      canvas.height = width / canvasAspectRatio;
      dy = (canvas.height - height) / 2;
    } else if (frameAspectRatio < canvasAspectRatio) {
      // Frame narrower than view: We need pillars on the sides
      canvas.width = height * canvasAspectRatio;
      canvas.height = height;
      dx = (canvas.width - width) / 2;
    } else {
      // Will stretch perfectly with no bars
      canvas.width = width;
      canvas.height = height;
    }

    if (dx > 0 || dy > 0) {
      context.fillStyle = 'black';
      context.fillRect(0, 0, canvas.width, canvas.height);
    }

    context.putImageData(
      new ImageData(new Uint8ClampedArray(this.buffer, 0, width * height * 4), width, height),
      dx,
      dy
    );
  }
}
