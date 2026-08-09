<script>
  let name = 'World';
  let count = 0;

  function increment() {
    count += 1;
  }
</script>

<h1>Hello, {name}!</h1>
<p>Count: {count}</p>
<button on:click={increment}>Increment</button>
