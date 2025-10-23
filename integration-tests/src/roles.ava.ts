import { createTest } from './setup.ts';

const test = createTest();

test('Check roles', async t => {
  console.log('🧪 Check roles');

  const { contract, issuer, executor } = t.context.accounts;

  console.log('  ➤ View contract.members(issuer)');
  let issuers: Array<string> = await contract.view('members', { role: 'issuer' });
  console.log('    ↩ Result:', issuers);

  t.is(issuers.length, 1);
  t.is(issuers.at(0), issuer.accountId);

  console.log('  ➤ View contract.members(executors)');
  let executors: Array<string> = await contract.view('members', { role: 'executor' });
  console.log('    ↩ Result:', executors);

  t.is(executors.length, 1);
  t.is(executors.at(0), executor.accountId);
});
