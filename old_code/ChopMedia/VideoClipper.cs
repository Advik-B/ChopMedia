using ImGuiNET;
using Microsoft.Xna.Framework;
using Microsoft.Xna.Framework.Graphics;
using Microsoft.Xna.Framework.Input;
using MonoGame.ImGuiNet;
using NativeFileDialogSharp;
using OpenCvSharp;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text.RegularExpressions;
using NAudio.Wave;

namespace ChopMedia
{
    public class VideoClipper : IDisposable
    {
        private readonly GraphicsDevice _graphicsDevice;
        private readonly ImGuiRenderer _imGuiRenderer;
        private string _videoPath = "";
        private VideoCapture? _capture;
        private double _fps;
        private int _frameCount, _videoWidth, _videoHeight;
        private int _startFrame, _endFrame, _currentFrame;
        private bool _isPlaying;
        private string _startTimeStr = "00:00:00.000";
        private string _endTimeStr = "00:00:00.000";
        private bool _frameAccurate = true;
        private bool _isCropping;
        private System.Numerics.Vector4 _cropRectNorm = new(0, 0, 1, 1);
        private Texture2D? _videoTexture;
        private IntPtr _videoTextureId;
        private readonly List<Texture2D> _thumbnailTextures = new();
        private readonly List<IntPtr> _thumbnailTextureIds = new();
        private const int NumThumbnails = 40;
        private readonly ConcurrentQueue<Action> _mainThreadActions = new();
        private CancellationTokenSource _cancellationTokenSource = new();
        private Task? _exportTask;
        private float _exportProgress;
        private string _exportMessage = "";
        private WaveOutEvent? _audioOut;
        private MediaFoundationReader? _audioReader;
        private float _volume = 1.0f;
        private float[]? _waveformSamples;
        private int _waveformSampleCount = 512;
        private string _statusMessage = "";
        private float _statusProgress = -1f;


        public VideoClipper(GraphicsDevice graphicsDevice, ImGuiRenderer imGuiRenderer)
        {
            _graphicsDevice = graphicsDevice;
            _imGuiRenderer = imGuiRenderer;
        }

        #region UI Drawing

        public void DrawUI()
        {
            var mainViewport = ImGui.GetMainViewport();
            ImGui.SetNextWindowPos(mainViewport.Pos);
            ImGui.SetNextWindowSize(mainViewport.Size);

            ImGui.Begin("Main", ImGuiWindowFlags.NoDecoration | ImGuiWindowFlags.NoMove | ImGuiWindowFlags.NoResize);

            var totalSize = ImGui.GetContentRegionAvail();
            const float timelineHeight = 60f;
            const float waveformHeight = 60f;
            const float controlPanelWidth = 320f;
            var topHeight = totalSize.Y - timelineHeight - waveformHeight - 100f;

            // Split horizontally: Preview | Controls
            ImGui.BeginChild("TopRow", totalSize with { Y = topHeight }, ImGuiChildFlags.Border | ImGuiChildFlags.AutoResizeY);
            ImGui.Columns(2, "TopRowColumns", false);
            ImGui.SetColumnWidth(0, totalSize.X - controlPanelWidth);

            DrawVideoPreview();
            ImGui.NextColumn();
            DrawControls();
            ImGui.EndChild();
            DrawTimeline();
            DrawWaveform();
            ImGui.End();
            DrawExportPopup();
        }

        public void ApplyCustomStyle()
{
    var style = ImGui.GetStyle();
    var colors = style.Colors;

    style.WindowRounding = 5f;
    style.FrameRounding = 4f;
    style.GrabRounding = 3f;
    style.ScrollbarRounding = 6f;
    style.TabRounding = 4f;

    colors[(int)ImGuiCol.Text]                   = new System.Numerics.Vector4(0.95f, 0.96f, 0.98f, 1.00f);
    colors[(int)ImGuiCol.WindowBg]               = new System.Numerics.Vector4(0.11f, 0.15f, 0.17f, 1.00f);
    colors[(int)ImGuiCol.ChildBg]                = new System.Numerics.Vector4(0.15f, 0.18f, 0.22f, 1.00f);
    colors[(int)ImGuiCol.Border]                 = new System.Numerics.Vector4(0.40f, 0.43f, 0.50f, 0.50f);
    colors[(int)ImGuiCol.FrameBg]                = new System.Numerics.Vector4(0.20f, 0.25f, 0.29f, 1.00f);
    colors[(int)ImGuiCol.FrameBgHovered]         = new System.Numerics.Vector4(0.12f, 0.20f, 0.28f, 1.00f);
    colors[(int)ImGuiCol.FrameBgActive]          = new System.Numerics.Vector4(0.09f, 0.12f, 0.14f, 1.00f);
    colors[(int)ImGuiCol.TitleBg]                = new System.Numerics.Vector4(0.09f, 0.12f, 0.14f, 1.00f);
    colors[(int)ImGuiCol.TitleBgActive]          = new System.Numerics.Vector4(0.08f, 0.10f, 0.12f, 1.00f);
    colors[(int)ImGuiCol.CheckMark]              = new System.Numerics.Vector4(0.28f, 0.56f, 1.00f, 1.00f);
    colors[(int)ImGuiCol.SliderGrab]             = new System.Numerics.Vector4(0.28f, 0.56f, 1.00f, 1.00f);
    colors[(int)ImGuiCol.SliderGrabActive]       = new System.Numerics.Vector4(0.37f, 0.61f, 1.00f, 1.00f);
    colors[(int)ImGuiCol.Button]                 = new System.Numerics.Vector4(0.20f, 0.25f, 0.29f, 1.00f);
    colors[(int)ImGuiCol.ButtonHovered]          = new System.Numerics.Vector4(0.28f, 0.56f, 1.00f, 1.00f);
    colors[(int)ImGuiCol.ButtonActive]           = new System.Numerics.Vector4(0.06f, 0.53f, 0.98f, 1.00f);
    colors[(int)ImGuiCol.Header]                 = new System.Numerics.Vector4(0.20f, 0.25f, 0.29f, 0.55f);
    colors[(int)ImGuiCol.HeaderHovered]          = new System.Numerics.Vector4(0.26f, 0.59f, 0.98f, 0.80f);
    colors[(int)ImGuiCol.HeaderActive]           = new System.Numerics.Vector4(0.26f, 0.59f, 0.98f, 1.00f);
}

        private void DrawWaveform()
        {
            ImGui.BeginChild("waveform", new System.Numerics.Vector2(0, 60), ImGuiChildFlags.Border);
            var drawList = ImGui.GetWindowDrawList();
            var wfStart = ImGui.GetCursorScreenPos();
            var wfSize = ImGui.GetContentRegionAvail();

            if (_waveformSamples != null && _waveformSamples.Length > 1)
            {
                float stepX = wfSize.X / _waveformSamples.Length;
                for (int i = 0; i < _waveformSamples.Length - 1; i++)
                {
                    float x1 = wfStart.X + i * stepX;
                    float y1 = wfStart.Y + (1 - _waveformSamples[i]) * wfSize.Y;
                    float x2 = wfStart.X + (i + 1) * stepX;
                    float y2 = wfStart.Y + (1 - _waveformSamples[i + 1]) * wfSize.Y;

                    drawList.AddLine(new System.Numerics.Vector2(x1, y1), new System.Numerics.Vector2(x2, y2),
                        ImGui.GetColorU32(new System.Numerics.Vector4(0.5f, 1f, 0.5f, 1)), 1f);
                }
            }
            ImGui.EndChild();
        }


        private void DrawVideoPreview()
        {
            ImGui.BeginChild("video_preview", new System.Numerics.Vector2(0, ImGui.GetContentRegionAvail().Y - 120),
                ImGuiChildFlags.Border | ImGuiChildFlags.ResizeY | ImGuiChildFlags.AutoResizeY);
            var availableSize = ImGui.GetContentRegionAvail();

            if (_capture != null && _videoTexture != null && _videoHeight > 0)
            {
                float aspect = (float)_videoWidth / _videoHeight;
                float imgW = availableSize.X;
                float imgH = imgW / aspect;

                if (imgH > availableSize.Y)
                {
                    imgH = availableSize.Y;
                    imgW = imgH * aspect;
                }

                var cursorPos = ImGui.GetCursorPos();
                ImGui.SetCursorPos(new System.Numerics.Vector2(
                    cursorPos.X + (availableSize.X - imgW) / 2,
                    cursorPos.Y + (availableSize.Y - imgH) / 2));

                ImGui.Image(_videoTextureId, new System.Numerics.Vector2(imgW, imgH));

                if (_isCropping)
                {
                    DrawCropOverlay(ImGui.GetItemRectMin(), ImGui.GetItemRectSize());
                }
            }

            ImGui.EndChild();
        }

        private void DrawCropOverlay(System.Numerics.Vector2 pos, System.Numerics.Vector2 size)
        {
            var drawList = ImGui.GetWindowDrawList();
            uint maskColor = ImGui.GetColorU32(new System.Numerics.Vector4(0, 0, 0, 0.5f));
            uint borderColor = ImGui.GetColorU32(new System.Numerics.Vector4(1, 1, 1, 1));

            float cropX = pos.X + _cropRectNorm.X * size.X;
            float cropY = pos.Y + _cropRectNorm.Y * size.Y;
            float cropW = _cropRectNorm.Z * size.X;
            float cropH = _cropRectNorm.W * size.Y;

            // Draw masking around crop
            drawList.AddRectFilled(pos, new System.Numerics.Vector2(pos.X + size.X, cropY), maskColor); // top
            drawList.AddRectFilled(new System.Numerics.Vector2(pos.X, cropY + cropH),
                new System.Numerics.Vector2(pos.X + size.X, pos.Y + size.Y), maskColor); // bottom
            drawList.AddRectFilled(new System.Numerics.Vector2(pos.X, cropY),
                new System.Numerics.Vector2(cropX, cropY + cropH), maskColor); // left
            drawList.AddRectFilled(new System.Numerics.Vector2(cropX + cropW, cropY),
                new System.Numerics.Vector2(pos.X + size.X, cropY + cropH), maskColor); // right

            // Draw crop border
            drawList.AddRect(new System.Numerics.Vector2(cropX, cropY),
                new System.Numerics.Vector2(cropX + cropW, cropY + cropH), borderColor, 0, ImDrawFlags.None, 2f);

            // Size of handle region (clickable)
            float gripSize = 12f;
            float normGripX = gripSize / size.X;
            float normGripY = gripSize / size.Y;

            // Define corner positions
            var corners = new (string id, float normX, float normY)[]
            {
                ("crop_tl", 0f, 0f),
                ("crop_tr", 1f, 0f),
                ("crop_bl", 0f, 1f),
                ("crop_br", 1f, 1f)
            };

            foreach (var (id, gx, gy) in corners)
            {
                var cx = cropX + gx * cropW;
                var cy = cropY + gy * cropH;

                // Draw L-shaped grip
                const float length = 10f;
                drawList.AddLine(new System.Numerics.Vector2(cx, cy),
                    new System.Numerics.Vector2(cx + (gx == 0 ? length : -length), cy), borderColor, 2f);
                drawList.AddLine(new System.Numerics.Vector2(cx, cy),
                    new System.Numerics.Vector2(cx, cy + (gy == 0 ? length : -length)), borderColor, 2f);

                // Handle interaction
                ImGui.SetCursorScreenPos(new System.Numerics.Vector2(cx - gripSize / 2, cy - gripSize / 2));
                ImGui.InvisibleButton(id, new System.Numerics.Vector2(gripSize, gripSize));
                if (ImGui.IsItemActive() && ImGui.IsMouseDragging(ImGuiMouseButton.Left))
                {
                    var delta = ImGui.GetMouseDragDelta();
                    ImGui.ResetMouseDragDelta();

                    float dx = delta.X / size.X;
                    float dy = delta.Y / size.Y;

                    if (gx == 0) // left
                    {
                        _cropRectNorm.X += dx;
                        _cropRectNorm.Z -= dx;
                    }
                    else // right
                    {
                        _cropRectNorm.Z += dx;
                    }

                    if (gy == 0) // top
                    {
                        _cropRectNorm.Y += dy;
                        _cropRectNorm.W -= dy;
                    }
                    else // bottom
                    {
                        _cropRectNorm.W += dy;
                    }

                    ClampCropRect();
                }
            }

            // Dragging entire crop area
            ImGui.SetCursorScreenPos(new System.Numerics.Vector2(cropX, cropY));
            ImGui.InvisibleButton("crop_drag", new System.Numerics.Vector2(cropW, cropH));
            if (ImGui.IsItemActive() && ImGui.IsMouseDragging(ImGuiMouseButton.Left))
            {
                var delta = ImGui.GetMouseDragDelta();
                ImGui.ResetMouseDragDelta();
                _cropRectNorm.X += delta.X / size.X;
                _cropRectNorm.Y += delta.Y / size.Y;
                ClampCropRect();
            }
        }
        
        private void ClampCropRect()
        {
            _cropRectNorm.X = Math.Clamp(_cropRectNorm.X, 0f, 1f);
            _cropRectNorm.Y = Math.Clamp(_cropRectNorm.Y, 0f, 1f);
            _cropRectNorm.Z = Math.Clamp(_cropRectNorm.Z, 0.05f, 1f - _cropRectNorm.X);
            _cropRectNorm.W = Math.Clamp(_cropRectNorm.W, 0.05f, 1f - _cropRectNorm.Y);
        }


        private void DrawTimeline()
        {
            ImGui.BeginChild("timeline", new System.Numerics.Vector2(0, 80), ImGuiChildFlags.Border);
            var w = ImGui.GetContentRegionAvail().X;
            var h = ImGui.GetContentRegionAvail().Y;
            var drawList = ImGui.GetWindowDrawList();
            var timelineStartPos = ImGui.GetCursorScreenPos();

            if (_thumbnailTextures.Count > 0)
            {
                float thumbW = w / _thumbnailTextures.Count;
                for (int i = 0; i < _thumbnailTextures.Count; i++)
                {
                    if (_thumbnailTextureIds[i] != IntPtr.Zero)
                    {
                        drawList.AddImage(_thumbnailTextureIds[i],
                            new System.Numerics.Vector2(timelineStartPos.X + i * thumbW, timelineStartPos.Y),
                            new System.Numerics.Vector2(timelineStartPos.X + (i + 1) * thumbW, timelineStartPos.Y + h));
                    }
                }
            }

            if (_frameCount > 0)
            {
                float startX = timelineStartPos.X + ((float)_startFrame / _frameCount) * w;
                float endX = timelineStartPos.X + ((float)_endFrame / _frameCount) * w;
                float playheadX = timelineStartPos.X + ((float)_currentFrame / _frameCount) * w;

                // Highlight trim range
                drawList.AddRectFilled(new System.Numerics.Vector2(startX, timelineStartPos.Y),
                    new System.Numerics.Vector2(endX, timelineStartPos.Y + h),
                    ImGui.GetColorU32(new System.Numerics.Vector4(0, 0.5f, 1, 0.3f)));

                // Draw playhead
                drawList.AddLine(new System.Numerics.Vector2(playheadX, timelineStartPos.Y),
                    new System.Numerics.Vector2(playheadX, timelineStartPos.Y + h),
                    ImGui.GetColorU32(new System.Numerics.Vector4(1, 1, 0, 1)), 2f);


                // Draw start and end handles with "I-beam" + circles
                void DrawHandle(string id, ref int frameRef, int minFrame, int maxFrame)
                {
                    float handleX = timelineStartPos.X + ((float)frameRef / _frameCount) * w;
                    float cx = handleX;
                    float cyTop = timelineStartPos.Y + 5;
                    float cyBot = timelineStartPos.Y + h - 5;
                    float r = 4f;

                    // Draw circles
                    drawList.AddCircleFilled(new System.Numerics.Vector2(cx, cyTop), r,
                        ImGui.GetColorU32(new System.Numerics.Vector4(1, 1, 1, 1)));
                    drawList.AddCircleFilled(new System.Numerics.Vector2(cx, cyBot), r,
                        ImGui.GetColorU32(new System.Numerics.Vector4(1, 1, 1, 1)));

                    // Draw line between them
                    drawList.AddLine(new System.Numerics.Vector2(cx, cyTop + r),
                        new System.Numerics.Vector2(cx, cyBot - r),
                        ImGui.GetColorU32(new System.Numerics.Vector4(1, 1, 1, 1)), 2f);

                    // Add interaction
                    ImGui.SetCursorScreenPos(timelineStartPos with { X = cx - r });
                    ImGui.InvisibleButton(id, new System.Numerics.Vector2(r * 2, h));
                    if (ImGui.IsItemActive() && ImGui.IsMouseDragging(ImGuiMouseButton.Left))
                    {
                        float deltaX = ImGui.GetMouseDragDelta().X;
                        ImGui.ResetMouseDragDelta();
                        int newFrame = (int)(((handleX + deltaX - timelineStartPos.X) / w) * _frameCount);
                        frameRef = Math.Clamp(newFrame, minFrame, maxFrame);
                        UpdateTimestamps();
                    }
                }

                DrawHandle("start_handle", ref _startFrame, 0, _endFrame - 1);
                DrawHandle("end_handle", ref _endFrame, _startFrame + 1, _frameCount - 1);

                // Playhead click interaction
                ImGui.SetCursorScreenPos(timelineStartPos);
                ImGui.InvisibleButton("timeline_click", new System.Numerics.Vector2(w, h));
                if (ImGui.IsItemHovered() && ImGui.IsMouseDown(ImGuiMouseButton.Left))
                {
                    float mouseX = ImGui.GetMousePos().X - timelineStartPos.X;
                    int newFrame = (int)((mouseX / w) * _frameCount);
                    if (newFrame != _currentFrame)
                    {
                        ShowFrame(newFrame);
                    }
                }
            }

            ImGui.EndChild();
        }


        private void DrawControls()
        {
            if (ImGui.Button("Load Video", new System.Numerics.Vector2(-1, 0)))
            {
                ShowOpenFileDialog();
            }

            if (_capture != null)
            {
                ImGui.Separator();
                if (ImGui.Checkbox("Enable Crop", ref _isCropping))
                {
                }

                if (_isCropping)
                {
                    ImGui.SameLine();
                    if (ImGui.Button("Reset Crop"))
                    {
                        _cropRectNorm = new System.Numerics.Vector4(0, 0, 1, 1);
                    }
                }

                ImGui.Text("Trim Controls");
                if (ImGui.InputText("Start", ref _startTimeStr, 32)) UpdateFromText();
                if (ImGui.InputText("End", ref _endTimeStr, 32)) UpdateFromText();

                ImGui.Separator();
                string playText = _isPlaying ? "Pause" : "Play";
                if (ImGui.Button(playText, new System.Numerics.Vector2(-1, 0)))
                {
                    TogglePlay();
                }

                ImGui.Separator();
                ImGui.Text("Export");
                ImGui.Checkbox("Frame-accurate (slower)", ref _frameAccurate);
                if (_isCropping)
                {
                    _frameAccurate = true;
                    ImGui.TextColored(new System.Numerics.Vector4(1, 0, 0, 1), "Note: Cropping requires re-encoding.");
                }

                if (ImGui.Button("Export Clip", new System.Numerics.Vector2(-1, 0)))
                {
                    if (_exportTask == null || _exportTask.IsCompleted)
                    {
                        ShowSaveFileDialog();
                    }
                }

                ImGui.Separator();
                ImGui.Text("Volume (Preview Only)");
                if (ImGui.SliderFloat("##volume", ref _volume, 0f, 1f))
                {
                    if (_audioOut != null)
                        _audioOut.Volume = _volume;
                }
            }
            ImGui.Separator();
            if (!string.IsNullOrEmpty(_statusMessage))
                ImGui.TextWrapped(_statusMessage);

            if (_statusProgress >= 0f && _statusProgress <= 1f)
            {
                ImGui.ProgressBar(_statusProgress, new System.Numerics.Vector2(-1, 0));
            }


        }

        private void DrawExportPopup()
        {
            if (_exportTask != null && !_exportTask.IsCompleted)
            {
                // Always open popup while export is in progress
                ImGui.OpenPopup("Exporting...");
            }

            bool popupOpen = true;
            if (ImGui.BeginPopupModal("Exporting...", ref popupOpen, ImGuiWindowFlags.AlwaysAutoResize))
            {
                ImGui.Text("Export in progress, please wait.");
                ImGui.ProgressBar(_exportProgress, new System.Numerics.Vector2(400, 0));
                ImGui.TextWrapped(_exportMessage);

                // Optional: allow user to cancel (will kill FFmpeg)
                if (ImGui.Button("Cancel"))
                {
                    _cancellationTokenSource.Cancel();
                }

                if (_exportTask!.IsCompleted)
                {
                    ImGui.Separator();
                    if (ImGui.Button("Close"))
                    {
                        ImGui.CloseCurrentPopup();
                        _exportMessage = "";
                    }
                }

                ImGui.EndPopup();
            }
        }


        #endregion

        #region Video & Texture Handling

        private void LoadVideo(string path)
        {
            if (!File.Exists(path)) return;
            ResetState();

            _videoPath = path;
            _capture = new VideoCapture(_videoPath);
            if (!_capture.IsOpened())
            {
                _capture.Dispose();
                _capture = null;
                return;
            }

            _fps = _capture.Get(VideoCaptureProperties.Fps);
            if (_fps <= 0) _fps = 30.0;
            _frameCount = (int)_capture.Get(VideoCaptureProperties.FrameCount);
            _videoWidth = (int)_capture.Get(VideoCaptureProperties.FrameWidth);
            _videoHeight = (int)_capture.Get(VideoCaptureProperties.FrameHeight);

            _startFrame = 0;
            _endFrame = _frameCount > 0 ? _frameCount - 1 : 0;
            _currentFrame = 0;

            if (_videoTexture != null) _imGuiRenderer.UnbindTexture(_videoTextureId);
            _videoTexture?.Dispose();
            _videoTexture = new Texture2D(_graphicsDevice, _videoWidth, _videoHeight, false, SurfaceFormat.Color);
            _videoTextureId = _imGuiRenderer.BindTexture(_videoTexture);

            try
            {
                _audioReader = new MediaFoundationReader(_videoPath);
                _audioOut = new WaveOutEvent();
                _audioOut.Init(_audioReader);
                _audioOut.Volume = _volume;
            }
            catch (Exception ex)
            {
                Console.WriteLine("Audio failed: " + ex.Message);
            }

            LoadThumbnails();
            UpdateTimestamps();
            ShowFrame(0);
            Task.Run(() => GenerateWaveform(), _cancellationTokenSource.Token);
        }

        private void GenerateWaveform()
        {
            try
            {
                using var reader = new AudioFileReader(_videoPath);
                int samplesPerChunk = (int)(reader.WaveFormat.SampleRate * reader.TotalTime.TotalSeconds /
                                            _waveformSampleCount);
                float[] buffer = new float[samplesPerChunk * reader.WaveFormat.Channels];
                _waveformSamples = new float[_waveformSampleCount];

                for (int i = 0; i < _waveformSampleCount; i++)
                {
                    int read = reader.Read(buffer, 0, buffer.Length);
                    if (read == 0) break;

                    float sum = 0;
                    for (int j = 0; j < read; j += reader.WaveFormat.Channels)
                    {
                        sum += Math.Abs(buffer[j]); // mono only for now
                    }

                    _waveformSamples[i] = sum / (read / reader.WaveFormat.Channels);
                }
            }
            catch (Exception ex)
            {
                Console.WriteLine("Waveform generation failed: " + ex.Message);
            }
        }


        private void LoadThumbnails()
        {
            _statusMessage = "Loading thumbnails...";
            _statusProgress = 0f;

            foreach (var id in _thumbnailTextureIds)
            {
                _imGuiRenderer.UnbindTexture(id);
            }

            foreach (var tex in _thumbnailTextures)
            {
                tex.Dispose();
            }

            _thumbnailTextures.Clear();
            _thumbnailTextureIds.Clear();

            for (int i = 0; i < NumThumbnails; i++)
            {
                var thumbTexture = new Texture2D(_graphicsDevice, 160, 90, false, SurfaceFormat.Color);
                var thumbId = _imGuiRenderer.BindTexture(thumbTexture);
                _thumbnailTextures.Add(thumbTexture);
                _thumbnailTextureIds.Add(thumbId);

                int frameNum = (int)(((float)i / NumThumbnails) * _frameCount);
                int index = i;
                Task.Run(() =>
                {
                    LoadSingleThumb(frameNum, thumbTexture);
                    _mainThreadActions.Enqueue(() =>
                    {
                        _statusProgress = (index + 1) / (float)NumThumbnails;
                        if (index + 1 == NumThumbnails)
                        {
                            _statusMessage = "Thumbnails loaded.";
                            _statusProgress = -1f;
                        }
                    });
                });
            }
        }

        private void DrawStatusBar()
        {
            ImGui.Begin("StatusBar",
                ImGuiWindowFlags.NoTitleBar | ImGuiWindowFlags.NoResize | ImGuiWindowFlags.NoMove |
                ImGuiWindowFlags.NoScrollbar);
            ImGui.SetWindowPos(new System.Numerics.Vector2(0, ImGui.GetIO().DisplaySize.Y - 30));
            ImGui.SetWindowSize(new System.Numerics.Vector2(ImGui.GetIO().DisplaySize.X, 30));

            ImGui.Text(_statusMessage);

            if (_statusProgress >= 0f && _statusProgress <= 1f)
            {
                ImGui.SameLine();
                ImGui.ProgressBar(_statusProgress, new System.Numerics.Vector2(200, 0));
            }

            ImGui.End();
        }


        private void LoadSingleThumb(int frameNum, Texture2D texture)
        {
            if (_cancellationTokenSource.IsCancellationRequested) return;
            using var cap = new VideoCapture(_videoPath);
            if (!cap.IsOpened()) return;

            cap.Set(VideoCaptureProperties.PosFrames, frameNum);
            using var frame = new Mat();
            if (cap.Read(frame) && !frame.Empty())
            {
                using var resized = new Mat();
                Cv2.Resize(frame, resized, new OpenCvSharp.Size(texture.Width, texture.Height));
                UpdateTextureFromMat(texture, resized);
            }
        }

        private void ShowFrame(int frameNum)
        {
            if (_capture == null || frameNum < 0 || frameNum >= _frameCount) return;
            _currentFrame = frameNum;

            Task.Run(() =>
            {
                if (_cancellationTokenSource.IsCancellationRequested) return;
                using var cap = new VideoCapture(_videoPath);
                if (!cap.IsOpened()) return;
                cap.Set(VideoCaptureProperties.PosFrames, frameNum);
                using var frame = new Mat();
                if (cap.Read(frame) && !frame.Empty())
                {
                    UpdateTextureFromMat(_videoTexture, frame);
                }
            }, _cancellationTokenSource.Token);
        }

        /// <summary>
        /// FIXED: This is the corrected implementation to prevent AccessViolationException.
        /// It extracts all required data from the Mat object and captures only the safe, managed data
        /// in the Action that will be executed on the main thread.
        /// </summary>
        private void UpdateTextureFromMat(Texture2D? texture, Mat frame)
        {
            if (texture == null || frame.Empty() || _cancellationTokenSource.IsCancellationRequested) return;

            using var matConverted = new Mat();
            Cv2.CvtColor(frame, matConverted, ColorConversionCodes.BGR2RGBA); // fixed color format

            int width = matConverted.Width;
            int height = matConverted.Height;
            int sizeInBytes = width * height * 4;
            var data = new byte[sizeInBytes];
            Marshal.Copy(matConverted.Data, data, 0, sizeInBytes);

            _mainThreadActions.Enqueue(() =>
            {
                if (!texture.IsDisposed && texture.Width == width && texture.Height == height)
                {
                    texture.SetData(data);
                }
            });
        }

        #endregion

        #region Logic & Control

        public void Update(GameTime gameTime, KeyboardState currentKeyboard, KeyboardState previousKeyboard)
        {
            while (_mainThreadActions.TryDequeue(out var action))
            {
                action();
            }

            HandleKeyboard(currentKeyboard, previousKeyboard);
        }

        private void TogglePlay()
        {
            _isPlaying = !_isPlaying;

            if (_isPlaying && _capture != null)
            {
                if (_audioReader != null)
                    _audioReader.Position = (long)TimeSpan.FromSeconds(_currentFrame / _fps).TotalMilliseconds;
                _audioOut?.Play();
                Task.Run(PlayVideo, _cancellationTokenSource.Token);
            }
            else
            {
                _audioOut?.Pause();
            }
        }


        private async Task PlayVideo()
        {
            var stopwatch = Stopwatch.StartNew();
            float initialElapsed = (_fps > 0) ? Math.Max(0, (_currentFrame - _startFrame) / (float)_fps) : 0;

            while (_isPlaying && _currentFrame < _endFrame)
            {
                if (_cancellationTokenSource.IsCancellationRequested) break;

                var elapsedSeconds = stopwatch.Elapsed.TotalSeconds + initialElapsed;
                if (_audioReader != null)
                    _audioReader.Position = (long)TimeSpan.FromSeconds(_currentFrame / _fps).TotalMilliseconds;
                int targetFrame = _startFrame + (int)(elapsedSeconds * _fps);

                if (targetFrame > _currentFrame)
                {
                    ShowFrame(targetFrame);
                }

                await Task.Delay(1);
            }

            _isPlaying = false;
        }

        private void HandleKeyboard(KeyboardState currentKeyboard, KeyboardState previousKeyboard)
        {
            var io = ImGui.GetIO();
            if (io.WantCaptureKeyboard) return;

            bool IsKeyPressed(Keys key) => currentKeyboard.IsKeyDown(key) && previousKeyboard.IsKeyUp(key);

            if (IsKeyPressed(Keys.Space)) TogglePlay();
            if (IsKeyPressed(Keys.Left)) ShowFrame(Math.Max(0, _currentFrame - 1));
            if (IsKeyPressed(Keys.Right)) ShowFrame(Math.Min(_frameCount - 1, _currentFrame + 1));

            if (currentKeyboard.IsKeyDown(Keys.I))
            {
                _startFrame = _currentFrame;
                UpdateTimestamps();
            }

            if (currentKeyboard.IsKeyDown(Keys.O))
            {
                _endFrame = _currentFrame;
                UpdateTimestamps();
            }
        }

        // ... (ExportClip, Helpers, and Dispose methods are unchanged) ...
        private void ExportClip(string outputPath)
        {
            if (string.IsNullOrWhiteSpace(outputPath) || _capture == null) return;
            _exportTask = Task.Run(() => RunExport(outputPath), _cancellationTokenSource.Token);
        }

        private async Task RunExport(string outputPath)
        {
            _exportProgress = 0;
            _exportMessage = "Starting export...";
            var startSec = TimecodeToSeconds(_startTimeStr);
            var endSec = TimecodeToSeconds(_endTimeStr);

            if (!startSec.HasValue || !endSec.HasValue || endSec <= startSec)
            {
                _exportMessage = "Error: Invalid start or end time.";
                return;
            }

            var durationTotal = endSec.Value - startSec.Value;
            bool isAccurateMode = _frameAccurate || _isCropping;

            var args = new List<string> { "-y" };
            args.AddRange(isAccurateMode
                ? new[] { "-i", $"\"{_videoPath}\"", "-ss", startSec.Value.ToString(CultureInfo.InvariantCulture) }
                : new[] { "-ss", startSec.Value.ToString(CultureInfo.InvariantCulture), "-i", $"\"{_videoPath}\"" });

            args.AddRange(isAccurateMode
                ? new[] { "-t", (endSec.Value - startSec.Value).ToString(CultureInfo.InvariantCulture) }
                : new[] { "-to", endSec.Value.ToString(CultureInfo.InvariantCulture) });


            var videoFilters = new List<string>();
            if (_isCropping)
            {
                int w = (int)(_cropRectNorm.Z * _videoWidth);
                int h = (int)(_cropRectNorm.W * _videoHeight);
                int x = (int)(_cropRectNorm.X * _videoWidth);
                int y = (int)(_cropRectNorm.Y * _videoHeight);
                videoFilters.Add($"crop={w}:{h}:{x}:{y}");
            }

            if (videoFilters.Count > 0)
            {
                args.AddRange(new[] { "-vf", string.Join(",", videoFilters) });
            }

            if (!isAccurateMode)
            {
                args.AddRange(new[] { "-c", "copy" });
            }
            else
            {
                args.AddRange(new[] { "-c:v", "libx264", "-c:a", "aac" });
            }

            args.Add($"\"{outputPath}\"");

            var processStartInfo = new ProcessStartInfo
            {
                FileName = "ffmpeg",
                Arguments = string.Join(" ", args),
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };

            try
            {
                using var process = Process.Start(processStartInfo);
                if (process == null) throw new Exception("Failed to start ffmpeg process.");

                var timeRegex = new Regex(@"time=(\d{2}):(\d{2}):(\d{2})\.(\d{2})");

                while (!process.StandardError.EndOfStream)
                {
                    if (_cancellationTokenSource.IsCancellationRequested)
                    {
                        process.Kill();
                        _exportMessage = "Export canceled.";
                        return;
                    }

                    string? line = await process.StandardError.ReadLineAsync();
                    if (line == null) continue;
                    _exportMessage = line;
                    var match = timeRegex.Match(line);
                    if (match.Success)
                    {
                        var ts = new TimeSpan(0, int.Parse(match.Groups[1].Value), int.Parse(match.Groups[2].Value),
                            int.Parse(match.Groups[3].Value), int.Parse(match.Groups[4].Value) * 10);
                        _exportProgress = (float)(ts.TotalSeconds / durationTotal);
                    }
                }

                await process.WaitForExitAsync(_cancellationTokenSource.Token);
                _exportProgress = 1;
                _exportMessage = $"Export finished! Saved to {outputPath}";
            }
            catch (Exception ex)
            {
                _exportMessage = $"Export failed: {ex.Message}. Is ffmpeg in your PATH?";
            }
        }

        #endregion

        #region Helpers

        private void UpdateTimestamps()
        {
            if (_fps > 0)
            {
                _startTimeStr = SecondsToTimecode(_startFrame / _fps);
                _endTimeStr = SecondsToTimecode(_endFrame / _fps);
            }
        }

        private void UpdateFromText()
        {
            var startSec = TimecodeToSeconds(_startTimeStr);
            if (startSec.HasValue) _startFrame = Math.Max(0, (int)(startSec.Value * _fps));

            var endSec = TimecodeToSeconds(_endTimeStr);
            if (endSec.HasValue) _endFrame = Math.Min(_frameCount - 1, (int)(endSec.Value * _fps));
        }

        private string SecondsToTimecode(double totalSeconds)
        {
            if (totalSeconds < 0) totalSeconds = 0;
            var time = TimeSpan.FromSeconds(totalSeconds);
            return time.ToString(@"hh\:mm\:ss\.fff");
        }

        private double? TimecodeToSeconds(string timecode)
        {
            if (TimeSpan.TryParse(timecode, CultureInfo.InvariantCulture, out var time))
            {
                return time.TotalSeconds;
            }

            return null;
        }

        private void ResetState()
        {
            _isPlaying = false;
            _cancellationTokenSource.Cancel();
            _cancellationTokenSource.Dispose();
            _cancellationTokenSource = new CancellationTokenSource();

            _capture?.Dispose();
            _capture = null;

            foreach (var id in _thumbnailTextureIds)
            {
                _imGuiRenderer.UnbindTexture(id);
            }

            foreach (var tex in _thumbnailTextures)
            {
                tex.Dispose();
            }

            _thumbnailTextures.Clear();
            _thumbnailTextureIds.Clear();

            if (_videoTexture != null) _imGuiRenderer.UnbindTexture(_videoTextureId);
            _videoTexture?.Dispose();
            _videoTexture = null;
            _videoTextureId = IntPtr.Zero;

            while (_mainThreadActions.TryDequeue(out _))
            {
            }

            _audioOut?.Stop();
            _audioOut?.Dispose();
            _audioReader?.Dispose();
            _audioOut = null;
            _audioReader = null;
        }

        public void Dispose()
        {
            ResetState();
            _cancellationTokenSource.Dispose();
        }

        private void ShowOpenFileDialog()
        {
            DialogResult result = Dialog.FileOpen("mp4,mkv,mov,avi",
                Environment.GetFolderPath(Environment.SpecialFolder.MyVideos));
            if (result.IsOk)
            {
                _mainThreadActions.Enqueue(() => LoadVideo(result.Path));
            }
        }

        private void ShowSaveFileDialog()
        {
            DialogResult result = Dialog.FileSave("mp4", Environment.GetFolderPath(Environment.SpecialFolder.MyVideos));
            if (result.IsOk)
            {
                string path = result.Path;
                if (!path.EndsWith(".mp4", StringComparison.OrdinalIgnoreCase))
                {
                    path += ".mp4";
                }

                _mainThreadActions.Enqueue(() => ExportClip(path));
            }
        }

        #endregion
    }
}