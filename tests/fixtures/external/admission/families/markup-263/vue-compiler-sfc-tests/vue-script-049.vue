<script setup>
        let bar = 1
        defineProps({
          foo: {
            default: () => bar
          }
        })
        </script>