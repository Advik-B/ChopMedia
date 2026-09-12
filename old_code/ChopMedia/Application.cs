using ImGuiNET;
using Microsoft.Xna.Framework;
using Microsoft.Xna.Framework.Input;
using MonoGame.ImGuiNet;
using Color = Microsoft.Xna.Framework.Color;

namespace ChopMedia
{
    public class Application : Game
    {
        private GraphicsDeviceManager _graphics;
        private ImGuiRenderer? _imGuiRenderer;
        private VideoClipper? _videoClipper;

        private KeyboardState _previousKeyboardState;
        private ImFontPtr _font;

        public Application()
        {
            _graphics = new GraphicsDeviceManager(this);
            Content.RootDirectory = "Content";
            IsMouseVisible = true;
            Window.AllowUserResizing = true;
        }

        protected override void Initialize()
        {
            _graphics.PreferredBackBufferWidth = 1280;
            _graphics.PreferredBackBufferHeight = 720;
            _graphics.ApplyChanges();

            _imGuiRenderer = new ImGuiRenderer(this);
            _videoClipper = new VideoClipper(GraphicsDevice, _imGuiRenderer);
            _videoClipper.ApplyCustomStyle();

            base.Initialize();
        }

        protected override void LoadContent()
        {
            ImGuiIOPtr io = ImGui.GetIO();
            io.Fonts.Clear();

            _font = io.Fonts.AddFontFromFileTTF("Content/Fonts/Inter-SemiBold.ttf", 18f);
            _imGuiRenderer?.RebuildFontAtlas();

            // SetWindowIcon("Content/Icons/App.png"); // Must be .png or .bmp
        }

        protected override void Update(GameTime gameTime)
        {
            var currentKeyboardState = Keyboard.GetState();

            if (GamePad.GetState(PlayerIndex.One).Buttons.Back == ButtonState.Pressed ||
                currentKeyboardState.IsKeyDown(Keys.Escape))
                Exit();

            _videoClipper?.Update(gameTime, currentKeyboardState, _previousKeyboardState);
            _previousKeyboardState = currentKeyboardState;

            base.Update(gameTime);
        }

        protected override void Draw(GameTime gameTime)
        {
            GraphicsDevice.Clear(new Color(0.2f, 0.2f, 0.2f));

            _imGuiRenderer?.BeginLayout(gameTime);
            ImGui.PushFont(_font);
            _videoClipper?.DrawUI();
            ImGui.PopFont();
            _imGuiRenderer?.EndLayout();

            base.Draw(gameTime);
        }

        protected override void OnExiting(object sender, EventArgs args)
        {
            _videoClipper?.Dispose();
            base.OnExiting(sender, args);
        }
    }
}
