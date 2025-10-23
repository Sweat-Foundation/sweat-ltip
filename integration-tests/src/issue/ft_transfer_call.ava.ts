import { Account, sweat } from "../types.ts";
import { createTest } from "../setup.ts";

const test = createTest();

test('Check issue with `ft_transfer_call` call', async t => {
  const { contract, ft, alice, bob, issuer } = t.context.accounts;

  console.log('\n👞 Step one');
  {
    console.log('  ➤ View contract.get_spare_balance');
    const spareBalance = await contract.view('get_spare_balance');
    console.log('    ↩ Result:', spareBalance);

    t.is(spareBalance, '0');
  }

  console.log('\n👞 Step two');
  {
    const amount = sweat(5_000);
    const msg = JSON.stringify({
      type: 'issue',
      data: {
        issue_at: 1761218300,
        grants: [[alice.accountId, amount.toString()]]
      }
    });

    console.log(`  ➤ Call ft.ft_transfer_call(${contract.accountId}, ${amount.toString()}, ${msg}) by unauthorized account`);
    const result = await alice.call(
      ft, 'ft_transfer_call',
      { receiver_id: contract.accountId, amount: amount.toString(), msg },
      { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
    );
    console.log('    ↩ Result:', result);

    t.is(result, '0');

    console.log('  ➤ View contract.get_account(alice)');
    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account, null);
  }

  console.log('\n👞 Step three');
  {
    const aliceAmount = sweat(1_000);
    const bobAmount = sweat(2_000);
    const amount = aliceAmount + bobAmount;
    const issue_at = 1761218300;
    const msg = JSON.stringify({
      type: 'issue',
      data: {
        issue_at,
        grants: [
          [alice.accountId, aliceAmount.toString()],
          [bob.accountId, bobAmount.toString()],
        ]
      }
    });

    console.log(`  ➤ Call ft.ft_transfer_call(${contract.accountId}, ${amount.toString()}, ${msg}) by authorized account (${issuer.accountId})`);
    const result = await issuer.call(
      ft, 'ft_transfer_call',
      { receiver_id: contract.accountId, amount: amount.toString(), msg },
      { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
    );
    console.log('    ↩ Result:', result);

    t.is(result, amount.toString());

    console.log('  ➤ View contract.get_account(alice)');
    const aliceAccount: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', aliceAccount);

    t.is(aliceAccount?.grants[issue_at]?.total_amount, aliceAmount.toString());

    console.log('  ➤ View contract.get_account(bob)');
    const bobAccount: Account | null = await contract.view('get_account', { account_id: bob.accountId });
    console.log('    ↩ Result:', bobAccount);

    t.is(bobAccount?.grants[issue_at]?.total_amount, bobAmount.toString());
  }

  console.log('\n👞 Step four');
  {
    const amount = sweat(5_000_000);
    const issue_at = 1761219000;
    const msg = JSON.stringify({
      type: 'issue',
      data: {
        issue_at,
        grants: [
          [alice.accountId, (amount / 2n).toString()],
          [bob.accountId, (amount / 2n + 100n).toString()],
        ]
      }
    });

    console.log(`  ➤ Call ft.ft_transfer_call(${contract.accountId}, ${amount.toString()}, ${msg}) by authorized account`);
    const result = await issuer.call(
      ft, 'ft_transfer_call',
      { receiver_id: contract.accountId, amount: amount.toString(), msg },
      { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
    );
    console.log('    ↩ Result:', result);

    t.is(result, '0');

    console.log('  ➤ View contract.get_account(alice)');
    const aliceAccount: Account = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', aliceAccount);

    t.is(Object.keys(aliceAccount?.grants).length, 1);

    console.log('  ➤ View contract.get_account(bob)');
    const bobAccount: Account = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', bobAccount);

    t.is(Object.keys(bobAccount?.grants).length, 1);
  }

  console.log('\n👞 Step five');
  {
    console.log('  ➤ View contract.get_spare_balance');
    const spareBalance = await contract.view('get_spare_balance');
    console.log('    ↩ Result:', spareBalance);

    t.is(spareBalance, '0');
  }
});
