using MagicalCryptoWallet.Helpers;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class LoggerTests
{
	[Theory]
	[InlineData("./Program.cs")]
	[InlineData("\\Program.cs")]
	[InlineData("Program.cs")]
	[InlineData("C:\\User\\user\\Github\\MagicalCryptoWallet\\MagicalCryptoWallet.Fluent.Desktop\\Program.cs")]
	[InlineData("/mnt/C/User/user/Github/nopara73/MagicalCryptoWallet.Fluent.Desktop/Program.cs")]
	[InlineData("~/Github/nopara73/MagicalCryptoWallet.Fluent.Desktop/Program.cs")]
	[InlineData("Program")]
	public void EndPointParserTests(string path)
	{
		var sourceFileName = EnvironmentHelpers.ExtractFileName(path);
		Assert.Equal("Program", sourceFileName);
	}
}
