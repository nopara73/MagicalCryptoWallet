using System.Collections.Generic;
using System.Linq;
using System.Reactive.Concurrency;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Threading;
using NBitcoin;
using ReactiveUI;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Models;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.AddWallet.Create;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Wallets;
using WabiSabi.Crypto.Randomness;
using Xunit;
using UiServices = MagicalCryptoWallet.Fluent.Services;

namespace MagicalCryptoWallet.Tests.UnitTests.ViewModels;

[Collection("Serial unit tests collection")]
public class ConfirmRecoveryWordsViewModelTests
{
	[Theory]
	[InlineData(12)]
	[InlineData(15)]
	[InlineData(18)]
	[InlineData(21)]
	[InlineData(24)]
	public void ChecksFourDistinctPositionsFromTheEntireMnemonic(int wordCount)
	{
		using var scope = new TestScope();
		var (backup, words) = CreateBackup(scope.Context, wordCount);
		var random = new ScriptedRandom(wordCount - 1, wordCount - 2, wordCount - 3, wordCount - 4);
		var model = new TestConfirmation(scope.Context, backup, words.AsEnumerable().Reverse().ToList(), random);
		model.OnNavigatedTo(false);
		try
		{
			Assert.Equal(Enumerable.Range(wordCount - 3, 4), model.ConfirmationWords.Select(x => x.Index));
			Assert.Equal(4, model.ConfirmationWords.Select(x => x.Index).Distinct().Count());
			Assert.All(model.ConfirmationWords, x => Assert.InRange(x.Index, 1, wordCount));
			Assert.Equal(words.Select(x => x.Word), model.AvailableWords.OrderBy(x => x.Index).Select(x => x.Word));
			Assert.Equal(wordCount, model.AvailableWords.Count);
			Assert.False(model.NextCommand!.CanExecute(null));
			Assert.Equal(4, random.Calls);
			for (int i = 0; i < 4; i++) { AnswerCurrentWord(model); }
			Assert.True(model.NextCommand.CanExecute(null));
			AssertBackupUntouched(words, backup);
		}
		finally
		{
			((INavigatable)model).OnNavigatedFrom(false);
		}
	}

	[Fact]
	public void DelayedUiBindingDoesNotDisableChoicesOrResetValidAnswers()
	{
		using var scope = new TestScope();
		var uiScheduler = new HistoricalScheduler();
		RxApp.MainThreadScheduler = uiScheduler;
		var (backup, words) = CreateBackup(scope.Context, 12);
		var model = new TestConfirmation(scope.Context, backup, words, new ScriptedRandom(8, 9, 10, 11));
		model.OnNavigatedTo(false);
		try
		{
			Assert.Empty(model.ConfirmationWords);
			Assert.All(model.AvailableWords, x => Assert.True(x.IsEnabled));
			Assert.False(model.NextCommand!.CanExecute(null));
			for (int i = 0; i < 4; i++) { AnswerCurrentWord(model); }
			uiScheduler.Start();
			Assert.Equal(new[] { 9, 10, 11, 12 }, model.ConfirmationWords.Select(x => x.Index));
			Assert.All(model.ConfirmationWords, x => Assert.True(x.IsConfirmed));
			Assert.True(model.AllWordsConfirmed);
			Assert.True(model.NextCommand.CanExecute(null));
			AssertBackupUntouched(words, backup);
		}
		finally
		{
			((INavigatable)model).OnNavigatedFrom(false);
		}
	}

	[Fact]
	public void WrongAndIncompleteAnswersBlockContinueUntilAllFourPositionsAreCorrect()
	{
		using var scope = new TestScope();
		var (backup, words) = CreateBackup(scope.Context, 12);
		var model = new TestConfirmation(scope.Context, backup, words, new ScriptedRandom(0, 1, 2, 3));
		model.OnNavigatedTo(false);
		try
		{
			var firstPrompt = model.CurrentWord;
			var wrong = model.AvailableWords.First(x => x.Word != firstPrompt.Word);
			wrong.IsSelected = true;
			Assert.Equal(wrong.Word, firstPrompt.SelectedWord);
			Assert.False(firstPrompt.IsConfirmed);
			Assert.Same(firstPrompt, model.CurrentWord);
			Assert.False(model.NextCommand!.CanExecute(null));
			Assert.All(model.AvailableWords.Where(x => x != wrong), x => Assert.False(x.IsEnabled));
			Assert.True(wrong.IsEnabled);

			wrong.IsSelected = false;
			Assert.Null(firstPrompt.SelectedWord);
			Assert.All(model.AvailableWords, x => Assert.True(x.IsEnabled));
			for (int answered = 1; answered <= 4; answered++)
			{
				AnswerCurrentWord(model);
				Assert.Equal(answered == 4, model.AllWordsConfirmed);
				Assert.Equal(answered == 4, model.NextCommand.CanExecute(null));
				Assert.Equal(answered, model.ConfirmationWords.Count(x => x.IsConfirmed));
			}
			Assert.All(model.AvailableWords, x => Assert.False(x.IsEnabled));
			AssertBackupUntouched(words, backup);
		}
		finally
		{
			((INavigatable)model).OnNavigatedFrom(false);
		}
	}

	[Fact]
	public void RepeatedWordValuesAreConfirmedAtTheirOriginalPositions()
	{
		using var scope = new TestScope();
		var mnemonic = new Mnemonic("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about");
		var backup = new WalletCreationOptions.AddNewWallet(new RecoveryWordsBackup("", mnemonic));
		var words = new RecoveryWordsViewModel(scope.Context, backup).MnemonicWords;
		var model = new TestConfirmation(scope.Context, backup, words, new ScriptedRandom(0, 3, 7, 11));
		model.OnNavigatedTo(false);
		try
		{
			Assert.Equal(new[] { 1, 4, 8, 12 }, model.ConfirmationWords.Select(x => x.Index));
			Assert.Equal(11, model.AvailableWords.Count(x => x.Word == "abandon"));
			// Choices are words, so any unused occurrence of the correct value can answer a position.
			for (int i = 0; i < 3; i++)
			{
				var choice = model.AvailableWords.Last(x => !x.IsConfirmed && x.Word == "abandon");
				choice.IsSelected = true;
				Assert.False(model.NextCommand!.CanExecute(null));
				// A consumed (disabled) choice cannot clear the next answer or advance it again.
				var current = model.CurrentWord;
				choice.IsSelected = false;
				Assert.Same(current, model.CurrentWord);
				Assert.Null(current.SelectedWord);
			}
			Assert.Equal(12, model.CurrentWord.Index);
			AnswerCurrentWord(model);
			Assert.True(model.NextCommand!.CanExecute(null));
			Assert.Equal(new[] { "abandon", "abandon", "abandon", "about" }, model.ConfirmationWords.Select(x => x.SelectedWord));
			AssertBackupUntouched(words, backup);
		}
		finally
		{
			((INavigatable)model).OnNavigatedFrom(false);
		}
	}

	[Theory]
	[InlineData(false, false)]
	[InlineData(false, true)]
	[InlineData(true, false)]
	[InlineData(true, true)]
	public void EveryVisitResamplesAndDropsAllPreviousAnswers(bool completeFirstVisit, bool returnFromHistory)
	{
		using var scope = new TestScope();
		var (backup, words) = CreateBackup(scope.Context, 12);
		var random = new ScriptedRandom(0, 1, 2, 3, 8, 9, 10, 11);
		var model = new TestConfirmation(scope.Context, backup, words, random);
		model.OnNavigatedTo(false);
		var oldChoices = model.AvailableWords;
		var oldPrompts = model.ConfirmationWords.ToArray();
		AnswerCurrentWord(model);
		if (completeFirstVisit)
		{
			for (int i = 0; i < 3; i++) { AnswerCurrentWord(model); }
		}
		else
		{
			oldChoices.First(x => !x.IsConfirmed && x.Word != model.CurrentWord.Word).IsSelected = true;
		}
		((INavigatable)model).OnNavigatedFrom(true);
		model.OnNavigatedTo(returnFromHistory);
		try
		{
			Assert.Equal(new[] { 9, 10, 11, 12 }, model.ConfirmationWords.Select(x => x.Index));
			Assert.Equal(8, random.Calls);
			Assert.Equal(9, model.CurrentWord.Index);
			Assert.False(model.AllWordsConfirmed);
			Assert.False(model.NextCommand!.CanExecute(null));
			Assert.All(model.ConfirmationWords, x => { Assert.False(x.IsConfirmed); Assert.Null(x.SelectedWord); });
			Assert.All(model.AvailableWords, x => { Assert.False(x.IsSelected); Assert.False(x.IsConfirmed); Assert.True(x.IsEnabled); });
			Assert.Equal(words.Count, model.AvailableWords.Count);
			Assert.Contains("#9", model.Caption);
			oldChoices.First(x => !x.IsSelected).IsSelected = true;
			oldPrompts[0].SelectedWord = oldPrompts[0].Word;
			Assert.All(model.ConfirmationWords, x => Assert.Null(x.SelectedWord));
			for (int i = 0; i < 4; i++) { AnswerCurrentWord(model); }
			Assert.True(model.NextCommand.CanExecute(null));
			AssertBackupUntouched(words, backup);
		}
		finally
		{
			((INavigatable)model).OnNavigatedFrom(false);
		}
	}

	private static void AnswerCurrentWord(ConfirmRecoveryWordsViewModel model) => model.AvailableWords
		.First(x => x.IsEnabled && !x.IsConfirmed && x.Word == model.CurrentWord.Word).IsSelected = true;

	private static (WalletCreationOptions.AddNewWallet Backup, List<RecoveryWordViewModel> Words) CreateBackup(UiContext context, int wordCount)
	{
		var mnemonic = new Mnemonic(Wordlist.English, Enumerable.Range(0, wordCount / 3 * 4).Select(x => (byte)x).ToArray());
		var options = new WalletCreationOptions.AddNewWallet(new RecoveryWordsBackup("", mnemonic));
		return (options, new RecoveryWordsViewModel(context, options).MnemonicWords);
	}

	private static void AssertBackupUntouched(List<RecoveryWordViewModel> words, WalletCreationOptions.AddNewWallet options)
	{
		var backup = Assert.IsType<RecoveryWordsBackup>(options.SelectedWalletBackup);
		Assert.Equal(backup.Mnemonic.Words, words.Select(x => x.Word));
		Assert.Equal(Enumerable.Range(1, words.Count), words.Select(x => x.Index));
		Assert.All(words, x => { Assert.False(x.IsConfirmed); Assert.False(x.IsSelected); Assert.Null(x.SelectedWord); });
	}

	private sealed class TestConfirmation(UiContext context, WalletCreationOptions.AddNewWallet options, List<RecoveryWordViewModel> words, WalletRandom random)
		: ConfirmRecoveryWordsViewModel(context, options, words, random);

	private sealed class ScriptedRandom(params int[] positions) : WalletRandom
	{
		public int Calls { get; private set; }
		public override int GetInt(int fromInclusive, int toExclusive)
		{
			Assert.True(Calls < positions.Length, "Every visit must draw exactly four new positions.");
			int result = positions[Calls++];
			Assert.InRange(result, fromInclusive, toExclusive - 1);
			return result;
		}
		public override void GetBytes(byte[] buffer) => throw new InvalidOperationException("Only position sampling is allowed.");
		public override void GetBytes(Span<byte> buffer) => throw new InvalidOperationException("Only position sampling is allowed.");
	}

	private sealed class TestScope : IDisposable
	{
		private readonly IScheduler _previousScheduler = RxApp.MainThreadScheduler;
		private readonly IScheduler _previousCommandScheduler = RxSchedulers.MainThreadScheduler;
		public TestScope()
		{
			RxApp.MainThreadScheduler = ImmediateScheduler.Instance;
			RxSchedulers.MainThreadScheduler = ImmediateScheduler.Instance;
			// Only HasWallet/GetNetwork are needed. No wallet, files, keys, or networking are initialized.
			var session = (WalletSession)RuntimeHelpers.GetUninitializedObject(typeof(WalletSession));
			typeof(WalletSession).GetField("_gate", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(session, Activator.CreateInstance(typeof(Lock)));
			SetProperty(session, nameof(WalletSession.Network), Network.RegTest);
			var services = (UiServices)RuntimeHelpers.GetUninitializedObject(typeof(UiServices));
			SetProperty(services, nameof(UiServices.WalletSession), session);
			var setup = (WalletSetupService)RuntimeHelpers.GetUninitializedObject(typeof(WalletSetupService));
			typeof(WalletSetupService).GetField("_services", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(setup, services);
			Context = (UiContext)RuntimeHelpers.GetUninitializedObject(typeof(UiContext));
			SetProperty(Context, nameof(UiContext.Services), services);
			SetProperty(Context, nameof(UiContext.WalletSetupService), setup);
		}
		public UiContext Context { get; }
		public void Dispose()
		{
			RxApp.MainThreadScheduler = _previousScheduler;
			RxSchedulers.MainThreadScheduler = _previousCommandScheduler;
		}
		private static void SetProperty(object target, string property, object value) => target.GetType()
			.GetField($"<{property}>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
	}
}
