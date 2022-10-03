//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import { RingRTC } from '../index';
import { Call } from './Service';

// Match a React.RefObject without relying on React.
interface Ref<T> {
  readonly current: T | null;
}

// Given a weird name to not conflict with WebCodec's VideoPixelFormat
export enum VideoPixelFormatEnum {
  I420 = 0,
  Nv12 = 1,
  Rgba = 2,
}

function videoPixelFormatFromEnum(
  format: VideoPixelFormatEnum
): VideoPixelFormat {
  switch (format) {
    case VideoPixelFormatEnum.I420: {
      return 'I420';
    }
    case VideoPixelFormatEnum.Nv12: {
      return 'NV12';
    }
    case VideoPixelFormatEnum.Rgba: {
      return 'RGBA';
    }
  }
}

function videoPixelFormatToEnum(
  format: VideoPixelFormat
): VideoPixelFormatEnum | undefined {
  switch (format) {
    case 'I420': {
      return VideoPixelFormatEnum.I420;
    }
    case 'NV12': {
      return VideoPixelFormatEnum.Nv12;
    }
    case 'RGBA': {
      return VideoPixelFormatEnum.Rgba;
    }
  }
}

// The way a CanvasVideoRender gets VideoFrames
export interface VideoFrameSource {
  // Fills in the given buffer and returns the width x height
  // or returns undefined if nothing was filled in because no
  // video frame was available.
  receiveVideoFrame(buffer: Buffer): [number, number] | undefined;
}

// Sends frames (after getting them from something like GumVideoCapturer, for example).
interface VideoFrameSender {
  sendVideoFrame(
    width: number,
    height: number,
    format: VideoPixelFormatEnum,
    buffer: Buffer
  ): void;
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
  private sender?: VideoFrameSender;
  private mediaStream?: MediaStream;
  private spawnedSenderRunning: boolean = false;
  private preferredDeviceId?: string;
  private updateLocalPreviewIntervalId?: any;

  constructor(defaultCaptureOptions: GumVideoCaptureOptions) {
    this.defaultCaptureOptions = defaultCaptureOptions;
  }

  capturing() {
    return this.captureOptions != undefined;
  }

  setLocalPreview(localPreview: Ref<HTMLVideoElement> | undefined) {
    const oldLocalPreview = this.localPreview?.current;
    if (oldLocalPreview) {
      oldLocalPreview.srcObject = null;
    }

    this.localPreview = localPreview;

    this.updateLocalPreviewSourceObject();

    // This is a dumb hack around the fact that sometimes the
    // this.localPreview.current is updated without a call
    // to setLocalPreview, in which case the local preview
    // won't be rendered.
    if (this.updateLocalPreviewIntervalId != undefined) {
      clearInterval(this.updateLocalPreviewIntervalId);
    }
    this.updateLocalPreviewIntervalId = setInterval(
      this.updateLocalPreviewSourceObject.bind(this),
      1000
    );
  }

  enableCapture(): void {
    // tslint:disable no-floating-promises
    this.startCapturing(this.defaultCaptureOptions);
  }

  enableCaptureAndSend(
    sender: VideoFrameSender,
    options?: GumVideoCaptureOptions
  ): void {
    // tslint:disable no-floating-promises
    this.startCapturing(options ?? this.defaultCaptureOptions);
    this.startSending(sender);
  }

  disable(): void {
    this.stopCapturing();
    this.stopSending();

    if (this.updateLocalPreviewIntervalId != undefined) {
      clearInterval(this.updateLocalPreviewIntervalId);
    }
    this.updateLocalPreviewIntervalId = undefined;
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
    const cameras = devices.filter(d => d.kind == 'videoinput');
    return cameras;
  }

  private getUserMedia(options: GumVideoCaptureOptions): Promise<MediaStream> {
    // TODO: Figure out a better way to make typescript accept "mandatory".
    let constraints: any = {
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
      constraints.video = {
        mandatory: {
          chromeMediaSource: 'desktop',
          chromeMediaSourceId: options.screenShareSourceId,
          maxWidth: options.maxWidth,
          maxHeight: options.maxHeight,
          maxFrameRate: options.maxFramerate,
        },
      };
    }
    return window.navigator.mediaDevices.getUserMedia(constraints);
  }

  private async startCapturing(options: GumVideoCaptureOptions): Promise<void> {
    if (this.capturing()) {
      RingRTC.logWarn('startCapturing(): already capturing');
      return;
    }
    RingRTC.logInfo(
      `startCapturing(): ${options.maxWidth}x${options.maxHeight}@${options.maxFramerate}`
    );
    this.captureOptions = options;
    try {
      // If we start/stop/start, we may have concurrent calls to getUserMedia,
      // which is what we want if we're switching from camera to screenshare.
      // But we need to make sure we deal with the fact that things might be
      // different after the await here.
      const mediaStream = await this.getUserMedia(options);
      // It's possible video was disabled, switched to screenshare, or
      // switched to a different camera while awaiting a response, in
      // which case we need to disable the camera we just accessed.
      if (this.captureOptions != options) {
        RingRTC.logWarn(
          'startCapturing(): different state after getUserMedia()'
        );
        for (const track of mediaStream.getVideoTracks()) {
          // Make the light turn off faster
          track.stop();
        }
        return;
      }

      this.mediaStream = mediaStream;
      if (
        !this.spawnedSenderRunning &&
        this.mediaStream != undefined &&
        this.sender != undefined
      ) {
        this.spawnSender(this.mediaStream, this.sender);
      }

      this.updateLocalPreviewSourceObject();
    } catch (e) {
      RingRTC.logError(`startCapturing(): ${e}`);

      // It's possible video was disabled, switched to screenshare, or
      // switched to a different camera while awaiting a response, in
      // which case we should reset the captureOptions if we set them.
      if (this.captureOptions == options) {
        // We couldn't open the camera.  Oh well.
        this.captureOptions = undefined;
      }
    }
  }

  private stopCapturing(): void {
    if (!this.capturing()) {
      RingRTC.logWarn('stopCapturing(): not capturing');
      return;
    }
    RingRTC.logInfo('stopCapturing()');
    this.captureOptions = undefined;
    if (!!this.mediaStream) {
      for (const track of this.mediaStream.getVideoTracks()) {
        // Make the light turn off faster
        track.stop();
      }
      this.mediaStream = undefined;
    }

    this.updateLocalPreviewSourceObject();
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

    if (!this.spawnedSenderRunning && this.mediaStream != undefined) {
      this.spawnSender(this.mediaStream, this.sender);
    }
  }

  private spawnSender(mediaStream: MediaStream, sender: VideoFrameSender) {
    const track = mediaStream.getVideoTracks()[0];
    if (track == undefined || this.spawnedSenderRunning) {
      return;
    }

    const reader = new MediaStreamTrackProcessor({
      track,
    }).readable.getReader();
    const buffer = Buffer.alloc(MAX_VIDEO_CAPTURE_BUFFER_SIZE);
    this.spawnedSenderRunning = true;
    (async () => {
      try {
        while (sender === this.sender && mediaStream == this.mediaStream) {
          const { done, value: frame } = await reader.read();
          if (done) {
            break;
          }
          if (!frame) {
            continue;
          }
          try {
            const format = videoPixelFormatToEnum(frame.format ?? 'I420');
            if (format == undefined) {
              RingRTC.logWarn(
                `Unsupported video frame format: ${frame.format}`
              );
              break;
            }
            frame.copyTo(buffer);
            sender.sendVideoFrame(
              frame.codedWidth,
              frame.codedHeight,
              format,
              buffer
            );
          } catch (e) {
            RingRTC.logError(`sendVideoFrame(): ${e}`);
          } finally {
            // This must be called for more frames to come.
            frame.close();
          }
        }
      } catch (e) {
        RingRTC.logError(`spawnSender(): ${e}`);
      } finally {
        reader.releaseLock();
      }
      this.spawnedSenderRunning = false;
    })();
  }

  private stopSending(): void {
    // The spawned sender should stop
    this.sender = undefined;
  }

  private updateLocalPreviewSourceObject(): void {
    if (!this.localPreview) {
      return;
    }
    const localPreview = this.localPreview.current;
    if (!localPreview) {
      return;
    }

    const { mediaStream = null } = this;

    if (localPreview.srcObject === mediaStream) {
      return;
    }

    if (mediaStream) {
      localPreview.srcObject = mediaStream;
      if (localPreview.width === 0) {
        localPreview.width = this.captureOptions!.maxWidth;
      }
      if (localPreview.height === 0) {
        localPreview.height = this.captureOptions!.maxHeight;
      }
    } else {
      localPreview.srcObject = null;
    }
  }
}

// We add 10% in each dimension to allow for things that are slightly wider or taller than 1080p.
const MAX_VIDEO_CAPTURE_MULTIPLIER = 1.0;
export const MAX_VIDEO_CAPTURE_WIDTH = 1920 * MAX_VIDEO_CAPTURE_MULTIPLIER;
export const MAX_VIDEO_CAPTURE_HEIGHT = 1080 * MAX_VIDEO_CAPTURE_MULTIPLIER;
export const MAX_VIDEO_CAPTURE_AREA =
  MAX_VIDEO_CAPTURE_WIDTH * MAX_VIDEO_CAPTURE_HEIGHT;
export const MAX_VIDEO_CAPTURE_BUFFER_SIZE = MAX_VIDEO_CAPTURE_AREA * 4;

export class CanvasVideoRenderer {
  private canvas?: Ref<HTMLCanvasElement>;
  private buffer: Buffer;
  private imageData?: ImageData;
  private source?: VideoFrameSource;
  private rafId?: any;

  constructor() {
    this.buffer = Buffer.alloc(MAX_VIDEO_CAPTURE_BUFFER_SIZE);
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
    this.rafId = window.requestAnimationFrame(
      this.requestAnimationFrameCallback.bind(this)
    );
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
    const [width, height] = frame;

    if (
      canvas.clientWidth <= 0 ||
      width <= 0 ||
      canvas.clientHeight <= 0 ||
      height <= 0
    ) {
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

    if (this.imageData?.width !== width || this.imageData?.height !== height) {
      this.imageData = new ImageData(width, height);
    }
    this.imageData.data.set(this.buffer.subarray(0, width * height * 4));
    context.putImageData(this.imageData, dx, dy);
  }
}
