
        <script>const bar = 1</script>
        <script setup>
        defineProps({
          foo: {
            default: () => bar
          }
        })
        </script>