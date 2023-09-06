package main

import (
	"fmt"
	"math/rand"
)

func main() {
	var beg, last, prog int64

	fmt.Print("Enter begin block: ")
	_, err := fmt.Scan(&beg)
	if err != nil {
		panic(fmt.Errorf("Error reading integer:", err))
	}
	fmt.Println("")

	fmt.Print("Enter end block: ")
	_, err = fmt.Scan(&last)
	if err != nil {
		panic(fmt.Errorf("Error reading integer:", err))
	}
	fmt.Println("")

	fmt.Print("Enter progress: ")
	_, err = fmt.Scan(&prog)
	if err != nil {
		panic(fmt.Errorf("Error reading integer:", err))
	}
	fmt.Println("")

	rsrc := rand.NewSource((beg << 32) + last)
	rnd := rand.New(rsrc)

	taskList := make([]int64, last-beg+1)
	for i := range taskList {
		taskList[i] = beg + int64(i)
	}

	rnd.Shuffle(len(taskList), func(i, j int) {
		tmp := taskList[j]
		taskList[j] = taskList[i]
		taskList[i] = tmp
	})

	// Prompt for third integer
	fmt.Println("The block is integer: ", taskList[prog])
}
