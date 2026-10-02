namespace MagicalCryptoWallet.WabiSabi.Client;

public interface IDestinationProvider
{
	IEnumerable<ScriptType> SupportedScriptTypes { get; }

	IEnumerable<IDestination> GetNextDestinations(int count, bool preferTaproot);

	public void TrySetScriptStates(KeyState state, IEnumerable<Script> scripts);
}
