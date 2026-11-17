<script setup>
        let bar = 1
        defineModel({
          get: () => bar,
          set: () => bar
        })
        </script>