using System;
using System.Collections.Generic;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Rpc;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
public sealed class SyntheticRpc : IJsonRpcService {
 [JsonRpcMethod("echo")] public string Echo(string text) => text;
 [JsonRpcMethod("optional")] public int Optional(int? fee = null, string? password = null) => (fee??0)+(password?.Length??0);
 [JsonRpcMethod("integer")] public int Integer(int number) => number;
 [JsonRpcMethod("long")] public long Long(long number) => number;
 [JsonRpcMethod("decimal")] public decimal Decimal(decimal number) => number;
 [JsonRpcMethod("money")] public long MoneyAmount(Money amount) => amount.Satoshi;
 [JsonRpcMethod("boolean")] public bool Boolean(bool enabled) => enabled;
 [JsonRpcMethod("guid")] public string GuidValue(Guid value) => value.ToString();
 [JsonRpcMethod("outpoint")] public OutPoint Outpoint(OutPoint value) => value;
 [JsonRpcMethod("shapes")] public Dictionary<string,object?> Shapes() => new() {
  ["long"]=9007199254740993L,["money"]=Money.Satoshis(1),["fee"]=new FeeRate(2.5m),
  ["hash"]=new uint256(new string('0',63)+"1"),["offset"]=new DateTimeOffset(2026,1,2,3,4,5,120,TimeSpan.FromHours(8)),
  ["utc"]=new DateTime(2026,1,2,3,4,5,DateTimeKind.Utc),["null"]=null,["bytes"]=new byte[]{0,1,2,255}
 };
 [JsonRpcMethod("fail")] public void Failure()=>throw new InvalidOperationException("synthetic domain failure");
 [JsonRpcMethod("async")] public async Task<int> Async()=>await Task.FromResult(7);
 [JsonRpcMethod("void")] public void Void() {}
 [JsonRpcMethod("payment")] public PaymentInfo[] Payment(PaymentInfo[] values)=>values;
 [JsonRpcMethod("address")] public BitcoinAddress Address(BitcoinAddress value)=>value;
 [JsonRpcMethod("destination")] public Destination DestinationValue(Destination value)=>value;
 [JsonRpcMethod("hash")] public uint256 Hash(uint256 value)=>value;
}
