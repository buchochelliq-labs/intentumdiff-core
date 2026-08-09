(defn greet [name]
  (str "Hello, " name))

(defn farewell [name]
  (str "Goodbye, " name))

(defn shout [phrase]
  (clojure.string/upper-case phrase))
