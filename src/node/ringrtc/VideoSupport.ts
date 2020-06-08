//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

import { Call } from './Service';

// Match a React.RefObject without relying on React.
interface Ref<T> {
  readonly current: T | null;
}

export class GumVideoCapturer {
  private readonly maxWidth: number;
  private readonly maxHeight: number;
  private readonly maxFramerate: number;
  private readonly localPreview: Ref<HTMLVideoElement>;
  private capturing: boolean;
  private call?: Call;
  private mediaStream?: MediaStream;
  private canvas?: OffscreenCanvas;
  private canvasContext?: OffscreenCanvasRenderingContext2D;
  private intervalId?: any;

  constructor(maxWidth: number, maxHeight: number, maxFramerate: number, localPreview: Ref<HTMLVideoElement>) {
    this.maxWidth = maxWidth;
    this.maxHeight = maxHeight;
    this.maxFramerate = maxFramerate;
    this.localPreview = localPreview;
    this.capturing =  false;
  }

  enableCapture(): void {
    // tslint:disable no-floating-promises
    this.startCapturing();
  }

  enableCaptureAndSend(call: Call): void {
    // tslint:disable no-floating-promises
    this.startCapturing();
    this.startSending(call);
  }

  disable(): void {
    this.stopCapturing();
    this.stopSending();
  }

  private async startCapturing(): Promise<void> {
    if (this.capturing) {
      return;
    }
    this.capturing = true;
    try {
      const devices = await window.navigator.mediaDevices.enumerateDevices();
      const filteredVideoInputDevices = devices.filter((device) => {
        return (device.kind == "videoinput" && !device.label.includes("IR Camera"));
      });
      const videoDeviceId = filteredVideoInputDevices.length == 0 ? undefined : filteredVideoInputDevices[0].deviceId;
      const mediaStream = await window.navigator.mediaDevices.getUserMedia({
        audio: false,
        video: {
          deviceId: videoDeviceId,
          width: {
            max: this.maxWidth,
          },
          height: {
            max: this.maxHeight,
          },
          frameRate: {
            max: this.maxFramerate,
          },
        },
      });
      // We could have been disabled between when we requested the stream
      // and when we got it.
      if (!this.capturing) {
        for (const track of mediaStream.getVideoTracks()) {
          // Make the light turn off faster
          track.stop();
        }
        return;
      }

      if (!!this.localPreview.current && !!mediaStream) {
        this.setLocalPreviewSourceObject(mediaStream);
      }
      this.mediaStream = mediaStream;
    } catch {
      // We couldn't open the camera.  Oh well.
    }
  }

  private stopCapturing(): void {
    if (!this.capturing) {
      return;
    }
    this.capturing = false;
    if (!!this.mediaStream) {
      for (const track of this.mediaStream.getVideoTracks()) {
        // Make the light turn off faster
        track.stop();
      }
      this.mediaStream = undefined;
    }
    if (!!this.localPreview.current) {
      this.localPreview.current.srcObject = null;
    }
  }

  private startSending(call: Call): void {
    if (this.call === call) {
      return;
    }
    this.call = call;
    this.canvas = new OffscreenCanvas(this.maxWidth, this.maxHeight);
    this.canvasContext = this.canvas.getContext('2d') || undefined;
    const interval = 1000 / this.maxFramerate;
    this.intervalId = setInterval(
      this.captureAndSendOneVideoFrame.bind(this),
      interval
    );
  }

  private stopSending(): void {
    this.call = undefined;
    this.canvas = undefined;
    this.canvasContext = undefined;
    if (!!this.intervalId) {
      clearInterval(this.intervalId);
    }
  }

  private setLocalPreviewSourceObject(mediaStream: MediaStream): void {
    const localPreview = this.localPreview.current;
    if (!localPreview) {
      return;
    }

    localPreview.srcObject = mediaStream;
    // I don't know why this is necessary
    if (localPreview.width === 0) {
      localPreview.width = this.maxWidth;
    }
    if (localPreview.height === 0) {
      localPreview.height = this.maxHeight;
    }
  }

  private captureAndSendOneVideoFrame(): void {
    if (!this.localPreview.current) {
      return;
    }
    if (!this.localPreview.current.srcObject && !!this.mediaStream) {
      this.setLocalPreviewSourceObject(this.mediaStream);
    }
    if (!this.canvas || !this.canvasContext || !this.call) {
      return;
    }
    const { width, height } = this.localPreview.current;
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
    this.call.sendVideoFrame(image.width, image.height, image.data.buffer);
  }
}

export class CanvasVideoRenderer {
  private readonly canvas: Ref<HTMLCanvasElement>;
  private buffer: ArrayBuffer;
  private call?: Call;
  private rafId?: any;

  constructor(canvas: Ref<HTMLCanvasElement>) {
    this.canvas = canvas;
    // The max size video frame we'll support (in RGBA)
    this.buffer = new ArrayBuffer(1920 * 1080 * 4);
  }

  enable(call: Call): void {
    if (this.call === call) {
      return;
    }
    this.call = call;
    this.requestAnimationFrameCallback();
  }

  disable() {
    this.renderBlack();
    this.call = undefined;
    if (this.rafId) {
      window.cancelAnimationFrame(this.rafId);
    }
  }

  private requestAnimationFrameCallback() {
    this.renderVideoFrame();
    this.rafId = window.requestAnimationFrame(this.requestAnimationFrameCallback.bind(this));
  }

  private renderBlack() {
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
    if (!this.call) {
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

    const frame = this.call.receiveVideoFrame(this.buffer);
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
