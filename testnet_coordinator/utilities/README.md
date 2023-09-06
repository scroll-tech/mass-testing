## Showprog: tell the corresponding chunk/batch id from current progress

For example if we found the progress range is `{xxx-yyy}` from `api/status`, we cal tell which block is responsed for the stuck progress `xxx`:

+ Input the begin/end index (which can be read in `config.yaml`)
+ Input the progress (`xxx`)

Now the utility would print the corresponding chunk /batch index and we can find which node it is assigned to in slack channel