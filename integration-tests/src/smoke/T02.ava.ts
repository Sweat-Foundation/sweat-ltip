import { createTest } from "../setup.ts";
import { Account, now, sweat } from "../types.ts";

const test = createTest(100, 900); // cliff_seconds = 100, vesting_seconds = 900
const STANDARD_GRANT_TOTAL = sweat(1_000);

test('T02 ? Issue creates grant and reduces spare', async t => {
  const { contract, ft, issuer, alice } = t.context.accounts;

  // Given: spare_balance = 1_000, alice has no grants
  const topUpAmount = STANDARD_GRANT_TOTAL;
  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: topUpAmount.toString(), msg: JSON.stringify({ type: 'top_up' }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );
  t.is(await contract.view('get_spare_balance'), topUpAmount.toString());

  // When: Issuer calls issue(account_id = alice, total_amount = 600, issue_date = T_ISSUE)
  const T_ISSUE = now();
  const grantAmount = sweat(600);
  await issuer.call(contract, 'issue', {
    issue_at: T_ISSUE,
    grants: [[alice.accountId, grantAmount.toString()]]
  });

  // Then: spare_balance = 400, alice has grant G1
  const expectedSpare = topUpAmount - grantAmount;
  t.is(await contract.view('get_spare_balance'), expectedSpare.toString());

  const account: Account = await contract.view('get_account', { account_id: alice.accountId });
  t.is(account.grants.length, 1);
  const G1 = account.grants.at(0)!;
  t.is(G1.issued_at, T_ISSUE);
  t.is(G1.total_amount, grantAmount.toString());
  t.is(G1.claimed_amount, '0');
  t.is(G1.order_amount, '0');
  t.is(G1.terminated_at, undefined);
});
