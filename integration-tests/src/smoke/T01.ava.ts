import { createTest } from "../setup.ts";
import { Account, ft_balance, now, sweat } from "../types.ts";

const test = createTest(100, 900); // cliff_seconds = 100, vesting_seconds = 900
const STANDARD_GRANT_TOTAL = sweat(1_000);

test('T01 ? TopUp increases spare balance', async t => {
  const { contract, ft, issuer } = t.context.accounts;

  // Given: spare_balance = 0, no grants exist
  t.is(await contract.view('get_spare_balance'), '0');

  // When: Issuer calls top_up(amount = 1_000)
  const topUpAmount = STANDARD_GRANT_TOTAL;
  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: topUpAmount.toString(), msg: JSON.stringify({ type: 'top_up' }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );

  // Then: spare_balance = 1_000, no grants created
  t.is(await contract.view('get_spare_balance'), topUpAmount.toString());
  const account: Account | null = await contract.view('get_account', { account_id: issuer.accountId });
  t.is(account, null);
});
