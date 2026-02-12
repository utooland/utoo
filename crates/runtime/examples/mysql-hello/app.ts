import mysql, { type Connection, type ResultSetHeader, type RowDataPacket } from 'mysql2/promise'

interface Member extends RowDataPacket {
  id: number
  name: string
  email: string
  role: string
  created_at: Date
}

async function main(): Promise<void> {
  console.log('=== MySQL CRUD Demo on utoo-runtime ===\n')

  const connection: Connection = await mysql.createConnection({
    host: '127.0.0.1',
    port: 3306,
    user: 'utoo',
    password: 'utoo',
    database: 'utoo_test',
  })
  console.log('Connected to MySQL!\n')

  // Clean up from previous runs
  await connection.execute('DELETE FROM members')

  // CREATE
  console.log('--- CREATE ---')
  const members: [string, string, string][] = [
    ['elrrrrrrr', 'elr@utoo.dev', 'runtime hacker'],
    ['xusd320', 'xusd320@utoo.dev', 'toolchain wizard'],
    ['zoomdong', 'zoomdong@utoo.dev', 'fullstack engineer'],
    ['killagu', 'killagu@utoo.dev', 'infra master'],
    ['claude', 'claude@utoo.dev', 'ai pair programmer'],
  ]
  for (const [name, email, role] of members) {
    const [result] = await connection.execute<ResultSetHeader>(
      'INSERT INTO members (name, email, role) VALUES (?, ?, ?)',
      [name, email, role],
    )
    console.log(`Inserted ${name} (id=${result.insertId})`)
  }

  // READ
  console.log('\n--- READ ---')
  const [rows] = await connection.execute<Member[]>('SELECT id, name, email, role FROM members ORDER BY id')
  for (const row of rows) {
    console.log(`  #${row.id} ${row.name} <${row.email}> - ${row.role}`)
  }

  // UPDATE
  console.log('\n--- UPDATE ---')
  await connection.execute('UPDATE members SET role = ? WHERE name = ?', ['mass yi lao shi', 'killagu'])
  const [updated] = await connection.execute<Member[]>('SELECT name, role FROM members WHERE name = ?', ['killagu'])
  console.log(`  ${updated[0].name} now: ${updated[0].role}`)

  // DELETE
  console.log('\n--- DELETE ---')
  await connection.execute('DELETE FROM members WHERE name = ?', ['claude'])
  const [remaining] = await connection.execute<Member[]>('SELECT name FROM members ORDER BY id')
  console.log(`  Remaining members: ${remaining.map((r) => r.name).join(', ')}`)

  await connection.end()
  console.log('\nDone! Connection closed.')
}

main().catch((err: Error) => {
  console.error('Error:', err.message)
  console.error('Stack:', err.stack)
  process.exit(1)
})
