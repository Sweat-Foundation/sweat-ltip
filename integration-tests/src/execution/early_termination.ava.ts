import { createTest } from "../setup";
import { Account, Grant, now, ONE_DAY_IN_SECONDS, sweat } from "../types";

const test = createTest();

test('🧪 Early termination', async t => {
  const { alice, issuer, executor, contract, ft } = t.context.accounts;

  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: sweat(1_000).toString(), msg: JSON.stringify({ type: "top_up" }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );

  const issue_at = now() - 300 * ONE_DAY_IN_SECONDS;
  await issuer.call(contract, 'issue', { issue_at, grants: [[alice.accountId, sweat(1_000).toString()]] });

  {
    await alice.call(contract, 'claim', {});

    const account: Account = await contract.view('get_account', { account_id: alice.accountId });
    t.is(account.grants.at(0)?.order_amount, '0');
  }

  {
    const terminate_at = now();
    await executor.call(contract, 'terminate', { account_id: alice.accountId, timestamp: terminate_at })

    const account: Account = await contract.view('get_account', { account_id: alice.accountId });
    const grant: Grant = account.grants.at(0)!;
    t.is(grant.order_amount, '0');
    t.is(grant.total_amount, '0');
    t.is(grant.claimed_amount, '0');
    t.is(grant.claimable_amount, '0');
    t.is(grant.terminated_at, terminate_at);
  }
});
