using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net.Mime;
using System.Runtime.InteropServices;
using System.Threading.Tasks;
using Avalonia.Platform.Storage;
using Avalonia.VisualTree;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class FileDialogHelper
{
	private static FilePickerFileType Json { get; } = new("JSON files")
	{
		Patterns = new[] { "*.json" },
		AppleUniformTypeIdentifiers = new[] { "public.json" },
		MimeTypes = new[] { MediaTypeNames.Application.Json }
	};

	private static FilePickerFileType Png { get; } = new("PNG files")
	{
		Patterns = new[] { "*.png" },
		AppleUniformTypeIdentifiers = new[] { "public.png" },
		MimeTypes = new[] { MediaTypeNames.Image.Png }
	};

	private static IStorageProvider? GetStorageProvider()
	{
		if (Application.Current?.ApplicationLifetime is IClassicDesktopStyleApplicationLifetime { MainWindow: { } window })
		{
			return window.StorageProvider;
		}

		if (Application.Current?.ApplicationLifetime is ISingleViewApplicationLifetime { MainView: { } mainView })
		{
			var visualRoot = mainView.GetVisualRoot();
			if (visualRoot is TopLevel topLevel)
			{
				return topLevel.StorageProvider;
			}
		}

		return null;
	}

	private static List<FilePickerFileType> GetFilePickerFileTypes(string[] filterExtTypes)
	{
		var fileTypeFilters = new List<FilePickerFileType>();

		foreach (var fileType in filterExtTypes)
		{
			switch (fileType)
			{
				case "json":
					{
						fileTypeFilters.Add(Json);
						break;
					}
				case "png":
					{
						fileTypeFilters.Add(Png);
						break;
					}
			}
		}

		return fileTypeFilters;
	}

	public static async Task<IStorageFile?> OpenFileAsync(string title, string[] filterExtTypes, string? directory = null)
	{
		var storageProvider = GetStorageProvider();
		if (storageProvider is null)
		{
			return null;
		}

		var suggestedStartLocation = await GetSuggestedStartLocationAsync(directory, storageProvider);

		var result = await storageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
		{
			Title = title,
			SuggestedStartLocation = suggestedStartLocation,
			FileTypeFilter = GetFilePickerFileTypes(filterExtTypes),
			AllowMultiple = false
		});

		return result.FirstOrDefault();
	}

	public static async Task<IStorageFile?> SaveFileAsync(string title, string[] filterExtTypes, string? initialFileName = null, string? directory = null)
	{
		var storageProvider = GetStorageProvider();
		if (storageProvider is null)
		{
			return null;
		}

		var suggestedStartLocation = await GetSuggestedStartLocationAsync(directory, storageProvider);

		return await storageProvider.SaveFilePickerAsync(new FilePickerSaveOptions
		{
			Title = title,
			FileTypeChoices = GetFilePickerFileTypes(filterExtTypes),
			SuggestedFileName = initialFileName,
			DefaultExtension = filterExtTypes.FirstOrDefault(),
			SuggestedStartLocation = suggestedStartLocation,
			ShowOverwritePrompt = true
		});
	}

	private static async Task<IStorageFolder?> GetSuggestedStartLocationAsync(string? directory, IStorageProvider storageProvider)
	{
		if (directory is not null)
		{
			return await storageProvider.TryGetFolderFromPathAsync(directory);
		}

		if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
		{
			return await storageProvider.TryGetFolderFromPathAsync(Path.Combine("/media", Environment.UserName));
		}

		if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
		{
			return await storageProvider.TryGetFolderFromPathAsync(Environment.GetFolderPath(Environment.SpecialFolder.Personal));
		}

		return null;
	}
}
