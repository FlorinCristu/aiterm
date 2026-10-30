require('http')
  .createServer((req, res) => res.end('hello from port 3300\n'))
  .listen(3300, () => console.log(`node server pid ${process.pid} listening on http://localhost:3300`));
