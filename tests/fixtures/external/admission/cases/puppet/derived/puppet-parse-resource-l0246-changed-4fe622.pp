notify { "thisisa[bug]": }
notify { 'other': }
Notify['other'] -> Notify["thisisa[bug]"]