import { createTest, fundAccounts, prepareFtContract, storageDeposit } from "../setup.ts";
import { hasError, sweat } from "../types.ts";

const test = createTest();

test('🧪 Check issue from invalid FT contract', async t => {
  const { issuer, root, contract, owner, ft } = t.context.accounts;

  console.log('🚢 Deploy invalid FT contract');
  const invalid_ft = await prepareFtContract(root, owner);

  await storageDeposit(invalid_ft, [contract, issuer]);
  await fundAccounts(invalid_ft, [issuer]);

  const result = await issuer.callRaw(
    invalid_ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: sweat(1_000_000).toString(), msg: JSON.stringify({ type: 'top_up' }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );
  t.assert(hasError(result, `Can only receive tokens from ${ft.accountId}`));
});
