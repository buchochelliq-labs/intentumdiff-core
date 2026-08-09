<template>
  <div>
    <h1>{{ greeting }}</h1>
    <button @click="changeGreeting">Change</button>
  </div>
</template>

<script>
export default {
  data() {
    return {
      greeting: 'Hello, World!',
      names: ['Alice', 'Bob', 'Carol'],
      current: 0
    }
  },
  methods: {
    changeGreeting() {
      this.current = (this.current + 1) % this.names.length;
      this.greeting = 'Hello, ' + this.names[this.current] + '!';
    }
  }
}
</script>
