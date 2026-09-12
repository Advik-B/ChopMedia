namespace ChopMedia
{
    internal class Program
    {
        [STAThread]
        static void Main(string[] args)
        {
            Environment.CurrentDirectory = AppContext.BaseDirectory;

            while (!Directory.Exists("Content") && Directory.GetParent(Environment.CurrentDirectory) != null)
            {
                Environment.CurrentDirectory = Directory.GetParent(Environment.CurrentDirectory)!.FullName;
            }

            using var game = new Application();
            game.Run();

        }
    }
}
