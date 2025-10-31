import { createTest } from "../setup";
import { Account, Grant, now, ONE_DAY_IN_SECONDS, sweat } from "../types";;

const test = createTest();

test('🧪 Continuous interaction', async t => {
  const { alice, issuer, executor, contract, ft } = t.context.accounts;

  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: sweat(500).toString(), msg: JSON.stringify({ type: "top_up" }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );

  const issue_at = 1717200000;
  await issuer.call(contract, 'issue', { issue_at, grants: [[alice.accountId, sweat(500).toString()]] });

  {
    const terminate_at = 1767198107;
    await executor.call(contract, 'terminate', { account_id: alice.accountId, timestamp: terminate_at })

    const account: Account = await contract.view('get_account', { account_id: alice.accountId });
    const grant = account.grants.at(0)!;
    t.is(grant.total_amount, '97571595425326051089');
    t.is(grant.terminated_at, terminate_at);
  }
});
