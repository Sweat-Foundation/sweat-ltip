import { createTest } from "../setup.ts";
import { Account, now, sweat } from "../types.ts";

const test = createTest(100, 900); // cliff_seconds = 100, vesting_seconds = 900
const STANDARD_GRANT_TOTAL = sweat(1_000);

test('T03 ? IssueWithTransfer uses incoming tokens + spare', async t => {
  const { contract, ft, issuer, bob } = t.context.accounts;

  // Given: spare_balance = 200, bob has no grants
  const initialSpare = sweat(200);
  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: initialSpare.toString(), msg: JSON.stringify({ type: 'top_up' }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );
  t.is(await contract.view('get_spare_balance'), initialSpare.toString());

  // When: Issuer calls ft_transfer_call with issue_with_transfer
  const T_ISSUE = now();
  const transferAmount = sweat(500);
  const grantTotal = sweat(600);
  const msg = JSON.stringify({
    type: 'issue',
    data: {
      issue_at: T_ISSUE,
      grants: [[bob.accountId, grantTotal.toString()]]
    }
  });

  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: transferAmount.toString(), msg },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );

  // Then: spare_balance = 100, bob has grant G1
  const expectedSpare = initialSpare + transferAmount - grantTotal;
  t.is(await contract.view('get_spare_balance'), expectedSpare.toString());

  const account: Account = await contract.view('get_account', { account_id: bob.accountId });
  t.is(account.grants.length, 1);
  const G1 = account.grants.at(0)!;
  t.is(G1.issued_at, T_ISSUE);
  t.is(G1.total_amount, grantTotal.toString());
  t.is(G1.claimed_amount, '0');
  t.is(G1.order_amount, '0');
  t.is(G1.terminated_at, undefined);
});
